use std::{
    error::Error,
    fmt,
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::Duration,
};

use monty::MontyRun;
use monty_types::{CompileOptions, MontyObject, NoLimitTracker, PrintWriter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ProbeSpec;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub enum WorkerError {
    Spawn(io::Error),
    Io(io::Error),
    Json(serde_json::Error),
    Timeout(Duration),
    Closed,
    Remote { error_type: String, message: String },
    Protocol(String),
}

impl fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "cannot start Python worker: {error}"),
            Self::Io(error) => write!(formatter, "Python worker I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "invalid Python worker JSON: {error}"),
            Self::Timeout(timeout) => {
                write!(
                    formatter,
                    "Python worker timed out after {} ms",
                    timeout.as_millis()
                )
            }
            Self::Closed => formatter.write_str("Python worker closed its output"),
            Self::Remote {
                error_type,
                message,
            } => write!(formatter, "Python worker raised {error_type}: {message}"),
            Self::Protocol(message) => write!(formatter, "Python worker protocol error: {message}"),
        }
    }
}

impl Error for WorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Spawn(error) | Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Timeout(_) | Self::Closed | Self::Remote { .. } | Self::Protocol(_) => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    result: Option<Value>,
    error_type: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Hello {
    kind: String,
    protocol: u64,
}

#[derive(Debug, Deserialize)]
struct GeneratorInfo {
    kind: String,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MinimizerInfo {
    kind: String,
    version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EnvironmentInfo {
    pub implementation: String,
    pub python_version: String,
    pub platform: String,
    pub generated_at: String,
}

#[derive(Debug, Deserialize)]
struct CatalogResponse {
    kind: String,
    probes: Vec<ProbeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OracleResponse {
    Return {
        value: Value,
        ast_nodes: Vec<String>,
    },
    Raise {
        error_type: String,
        error_message: String,
        ast_nodes: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MontyResponse {
    Return {
        value: Value,
        is_none: bool,
        repr: String,
    },
    CompileError {
        error_type: String,
        error_message: String,
    },
    RuntimeError {
        error_type: String,
        error_message: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GenerateResponse {
    Prepared {
        seed: u64,
        source: String,
        source_sha256: String,
        inert_source: String,
        ast_nodes: Vec<String>,
        ast_node_count: usize,
    },
    GenerationError {
        seed: u64,
        source: Option<String>,
        source_sha256: Option<String>,
        error_type: String,
        error_message: String,
    },
    GuardRejected {
        seed: u64,
        source: Option<String>,
        source_sha256: Option<String>,
        error_type: String,
        error_message: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct MinimizeCandidate {
    pub candidate_id: u64,
    pub inert_source: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MinimizeResponse {
    Minimized {
        source: String,
        source_sha256: String,
        ast_nodes: Vec<String>,
        ast_node_count: usize,
        checks: u64,
    },
    Unchanged {
        checks: u64,
    },
    MinimizationError {
        checks: u64,
        error_type: String,
        error_message: String,
    },
}

struct WorkerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Result<String, io::Error>>,
    reader: Option<JoinHandle<()>>,
    timeout: Duration,
    stopped: bool,
}

impl WorkerProcess {
    fn start(mut command: Command, timeout: Duration) -> Result<Self, WorkerError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(WorkerError::Spawn)?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WorkerError::Protocol(
                "spawned worker has no stdin pipe".to_owned(),
            ));
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WorkerError::Protocol(
                "spawned worker has no stdout pipe".to_owned(),
            ));
        };
        let (sender, responses) = mpsc::channel();
        let reader = match thread::Builder::new()
            .name("monty-discovery-python-reader".to_owned())
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                loop {
                    match read_bounded_line(&mut stdout, MAX_RESPONSE_BYTES) {
                        Ok(Some(line)) => {
                            if sender.send(Ok(line)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            break;
                        }
                    }
                }
            }) {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(WorkerError::Spawn(error));
            }
        };
        Ok(Self {
            child,
            stdin: Some(stdin),
            responses,
            reader: Some(reader),
            timeout,
            stopped: false,
        })
    }

    fn send<T: Serialize>(&mut self, request: &T) -> Result<(), WorkerError> {
        if self.stopped {
            return Err(WorkerError::Closed);
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(WorkerError::Closed);
        };
        serde_json::to_writer(&mut *stdin, request).map_err(WorkerError::Json)?;
        stdin.write_all(b"\n").map_err(WorkerError::Io)?;
        stdin.flush().map_err(WorkerError::Io)?;
        Ok(())
    }

    fn receive(&mut self) -> Result<Value, WorkerError> {
        let line = match self.responses.recv_timeout(self.timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => return Err(WorkerError::Io(error)),
            Err(RecvTimeoutError::Timeout) => {
                self.terminate();
                return Err(WorkerError::Timeout(self.timeout));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(WorkerError::Closed),
        };
        serde_json::from_str(&line).map_err(WorkerError::Json)
    }

    fn envelope(value: Value) -> Result<Value, WorkerError> {
        let envelope: Envelope = serde_json::from_value(value).map_err(WorkerError::Json)?;
        if envelope.ok {
            envelope.result.ok_or_else(|| {
                WorkerError::Protocol("successful response has no result".to_owned())
            })
        } else {
            Err(WorkerError::Remote {
                error_type: envelope
                    .error_type
                    .unwrap_or_else(|| "WorkerError".to_owned()),
                message: envelope.error_message.unwrap_or_default(),
            })
        }
    }

    fn request<T: Serialize>(&mut self, request: &T) -> Result<Value, WorkerError> {
        self.send(request)?;
        Self::envelope(self.receive()?)
    }

    fn request_interactive<T, F>(
        &mut self,
        request: &T,
        mut on_candidate: F,
    ) -> Result<Value, WorkerError>
    where
        T: Serialize,
        F: FnMut(&MinimizeCandidate) -> Result<bool, WorkerError>,
    {
        self.send(request)?;
        loop {
            let value = self.receive()?;
            if value.get("event").and_then(Value::as_str) == Some("minimize_candidate") {
                let candidate: MinimizeCandidate =
                    serde_json::from_value(value).map_err(WorkerError::Json)?;
                let preserves = match on_candidate(&candidate) {
                    Ok(preserves) => preserves,
                    Err(error) => {
                        self.terminate();
                        return Err(error);
                    }
                };
                self.send(&serde_json::json!({
                    "op": "minimize_verdict",
                    "candidate_id": candidate.candidate_id,
                    "preserves": preserves,
                }))?;
                continue;
            }
            return Self::envelope(value);
        }
    }

    fn terminate(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub struct PythonWorker {
    process: WorkerProcess,
    generator_version: Option<String>,
    generator_loaded: bool,
    minimizer_version: Option<String>,
    minimizer_loaded: bool,
}

impl PythonWorker {
    pub fn start(python: &str, timeout: Duration) -> Result<Self, WorkerError> {
        let mut command = Command::new(python);
        command.args(["-I", "-m", "monty_compat._discovery_worker"]);
        let mut process = WorkerProcess::start(command, timeout)?;
        let value = process.request(&serde_json::json!({"op": "hello"}))?;
        let hello: Hello = serde_json::from_value(value).map_err(WorkerError::Json)?;
        if hello.kind != "hello" || hello.protocol != 1 {
            return Err(WorkerError::Protocol(format!(
                "expected hello protocol 1, received kind {:?} protocol {}",
                hello.kind, hello.protocol
            )));
        }
        Ok(Self {
            process,
            generator_version: None,
            generator_loaded: false,
            minimizer_version: None,
            minimizer_loaded: false,
        })
    }

    pub fn load_generator(&mut self) -> Result<Option<&str>, WorkerError> {
        if !self.generator_loaded {
            let value = self
                .process
                .request(&serde_json::json!({"op": "generator_info"}))?;
            let info: GeneratorInfo = serde_json::from_value(value).map_err(WorkerError::Json)?;
            if info.kind != "generator_info" {
                return Err(WorkerError::Protocol(format!(
                    "expected generator_info, received {:?}",
                    info.kind
                )));
            }
            self.generator_version = info.version;
            self.generator_loaded = true;
        }
        Ok(self.generator_version.as_deref())
    }

    pub fn environment_info(&mut self) -> Result<EnvironmentInfo, WorkerError> {
        let value = self
            .process
            .request(&serde_json::json!({"op": "environment_info"}))?;
        let kind = value.get("kind").and_then(Value::as_str);
        if kind != Some("environment_info") {
            return Err(WorkerError::Protocol(format!(
                "expected environment_info, received {kind:?}"
            )));
        }
        serde_json::from_value(value).map_err(WorkerError::Json)
    }

    pub fn load_minimizer(&mut self) -> Result<Option<&str>, WorkerError> {
        if !self.minimizer_loaded {
            let value = self
                .process
                .request(&serde_json::json!({"op": "minimizer_info"}))?;
            let info: MinimizerInfo = serde_json::from_value(value).map_err(WorkerError::Json)?;
            if info.kind != "minimizer_info" {
                return Err(WorkerError::Protocol(format!(
                    "expected minimizer_info, received {:?}",
                    info.kind
                )));
            }
            self.minimizer_version = info.version;
            self.minimizer_loaded = true;
        }
        Ok(self.minimizer_version.as_deref())
    }

    pub fn catalog(&mut self) -> Result<Vec<ProbeSpec>, WorkerError> {
        let value = self
            .process
            .request(&serde_json::json!({"op": "catalog"}))?;
        let response: CatalogResponse = serde_json::from_value(value).map_err(WorkerError::Json)?;
        if response.kind != "catalog" {
            return Err(WorkerError::Protocol(format!(
                "expected catalog, received {:?}",
                response.kind
            )));
        }
        Ok(response.probes)
    }

    pub fn oracle(&mut self, source: &str) -> Result<OracleResponse, WorkerError> {
        let value = self.process.request(&serde_json::json!({
            "op": "oracle",
            "source": source,
        }))?;
        serde_json::from_value(value).map_err(WorkerError::Json)
    }

    pub fn generate<T: Serialize>(
        &mut self,
        seed: u64,
        config: &T,
    ) -> Result<GenerateResponse, WorkerError> {
        let value = self.process.request(&serde_json::json!({
            "op": "generate",
            "seed": seed,
            "config": config,
        }))?;
        serde_json::from_value(value).map_err(WorkerError::Json)
    }

    pub fn minimize<T, F>(
        &mut self,
        source: &str,
        config: &T,
        max_checks: u64,
        on_candidate: F,
    ) -> Result<MinimizeResponse, WorkerError>
    where
        T: Serialize,
        F: FnMut(&MinimizeCandidate) -> Result<bool, WorkerError>,
    {
        let value = self.process.request_interactive(
            &serde_json::json!({
                "op": "minimize",
                "source": source,
                "config": config,
                "max_checks": max_checks,
            }),
            on_candidate,
        )?;
        serde_json::from_value(value).map_err(WorkerError::Json)
    }
}

pub struct MontyWorker {
    process: WorkerProcess,
}

impl MontyWorker {
    pub fn start(executable: &Path, timeout: Duration) -> Result<Self, WorkerError> {
        let mut command = Command::new(executable);
        command.arg("--monty-worker");
        Ok(Self {
            process: WorkerProcess::start(command, timeout)?,
        })
    }

    pub fn run(&mut self, source: &str) -> Result<MontyResponse, WorkerError> {
        let value = self.process.request(&serde_json::json!({
            "op": "run",
            "source": source,
        }))?;
        serde_json::from_value(value).map_err(WorkerError::Json)
    }
}

pub fn run_monty_worker_stdio() -> Result<(), WorkerError> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    while let Some(line) =
        read_bounded_line(&mut input, MAX_RESPONSE_BYTES).map_err(WorkerError::Io)?
    {
        let request: Value = serde_json::from_str(&line).map_err(WorkerError::Json)?;
        let response = match request.get("op").and_then(Value::as_str) {
            Some("run") => {
                let source = request
                    .get("source")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        WorkerError::Protocol("Monty run request has no string source".to_owned())
                    })?;
                monty_response(source)
            }
            Some(operation) => {
                return Err(WorkerError::Protocol(format!(
                    "unknown Monty worker operation {operation:?}"
                )));
            }
            None => {
                return Err(WorkerError::Protocol(
                    "Monty worker request has no operation".to_owned(),
                ));
            }
        };
        let envelope = serde_json::json!({"ok": true, "result": response});
        let encoded = serde_json::to_vec(&envelope).map_err(WorkerError::Json)?;
        if encoded.len() > MAX_RESPONSE_BYTES {
            return Err(WorkerError::Protocol(
                "Monty worker response exceeds the byte limit".to_owned(),
            ));
        }
        output.write_all(&encoded).map_err(WorkerError::Io)?;
        output.write_all(b"\n").map_err(WorkerError::Io)?;
        output.flush().map_err(WorkerError::Io)?;
    }
    Ok(())
}

fn monty_response(source: &str) -> MontyResponse {
    let runner = match MontyRun::new(
        source.to_owned(),
        "<monty-capability-probe>",
        Vec::new(),
        CompileOptions::default(),
    ) {
        Ok(runner) => runner,
        Err(error) => {
            return MontyResponse::CompileError {
                error_type: error.exc_type().to_string(),
                error_message: error.message().unwrap_or_default().to_owned(),
            };
        }
    };
    let mut printed = String::new();
    match runner.run(
        Vec::new(),
        NoLimitTracker,
        PrintWriter::collect_string(&mut printed),
    ) {
        Ok(value) => MontyResponse::Return {
            value: crate::monty_wire_json_safe(&value),
            is_none: matches!(value, MontyObject::None),
            repr: value.py_repr(),
        },
        Err(error) => MontyResponse::RuntimeError {
            error_type: error.exc_type().to_string(),
            error_message: error.message().unwrap_or_default().to_owned(),
        },
    }
}

fn read_bounded_line(reader: &mut impl BufRead, max_bytes: usize) -> io::Result<Option<String>> {
    let mut output = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if output.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "worker response ended without a newline",
            ));
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if output.len().saturating_add(consumed) > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "worker response exceeds the configured byte limit",
            ));
        }
        output.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            if output.last() == Some(&b'\n') {
                output.pop();
            }
            return String::from_utf8(output)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_accepts_one_line_and_rejects_oversize() {
        let mut valid = io::Cursor::new(b"{\"ok\":true}\n".as_slice());
        assert_eq!(
            read_bounded_line(&mut valid, 32).expect("bounded fixture should read"),
            Some("{\"ok\":true}".to_owned())
        );

        let mut oversized = io::Cursor::new(b"123456\n".as_slice());
        assert!(read_bounded_line(&mut oversized, 4).is_err());
    }
}
