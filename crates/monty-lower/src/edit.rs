use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    InvalidRange {
        start: usize,
        end: usize,
        source_len: usize,
    },
    NonCharacterBoundary {
        offset: usize,
    },
    Conflict {
        first: (usize, usize),
        second: (usize, usize),
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange {
                start,
                end,
                source_len,
            } => write!(
                formatter,
                "invalid source edit range {start}..{end} for source length {source_len}"
            ),
            Self::NonCharacterBoundary { offset } => {
                write!(
                    formatter,
                    "source edit offset {offset} is not a UTF-8 boundary"
                )
            }
            Self::Conflict { first, second } => write!(
                formatter,
                "overlapping source edits at {}..{} and {}..{}",
                first.0, first.1, second.0, second.1
            ),
        }
    }
}

impl Error for EditError {}

pub(crate) fn apply_edits(source: &str, mut edits: Vec<Edit>) -> Result<String, EditError> {
    for edit in &edits {
        if edit.start > edit.end || edit.end > source.len() {
            return Err(EditError::InvalidRange {
                start: edit.start,
                end: edit.end,
                source_len: source.len(),
            });
        }
        for offset in [edit.start, edit.end] {
            if !source.is_char_boundary(offset) {
                return Err(EditError::NonCharacterBoundary { offset });
            }
        }
    }

    edits.sort_by_key(|edit| (edit.start, edit.end));
    for pair in edits.windows(2) {
        if let [first, second] = pair
            && second.start < first.end
        {
            return Err(EditError::Conflict {
                first: (first.start, first.end),
                second: (second.start, second.end),
            });
        }
    }

    let extra_capacity = edits
        .iter()
        .map(|edit| {
            edit.replacement
                .len()
                .saturating_sub(edit.end.saturating_sub(edit.start))
        })
        .fold(0usize, usize::saturating_add);
    let mut output = String::with_capacity(source.len().saturating_add(extra_capacity));
    let mut cursor = 0usize;
    for edit in edits {
        let Some(unchanged) = source.get(cursor..edit.start) else {
            return Err(EditError::NonCharacterBoundary { offset: edit.start });
        };
        output.push_str(unchanged);
        output.push_str(&edit.replacement);
        cursor = edit.end;
    }
    let Some(tail) = source.get(cursor..) else {
        return Err(EditError::NonCharacterBoundary { offset: cursor });
    };
    output.push_str(tail);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_edits_without_shifting_offsets() -> Result<(), EditError> {
        let result = apply_edits(
            "abcdef",
            vec![
                Edit {
                    start: 1,
                    end: 3,
                    replacement: "X".to_owned(),
                },
                Edit {
                    start: 5,
                    end: 6,
                    replacement: "Y".to_owned(),
                },
            ],
        )?;
        assert_eq!(result, "aXdeY");
        Ok(())
    }

    #[test]
    fn rejects_invalid_and_non_utf8_ranges_without_panicking() {
        assert!(matches!(
            apply_edits(
                "aé",
                vec![Edit {
                    start: 2,
                    end: 3,
                    replacement: String::new(),
                }]
            ),
            Err(EditError::NonCharacterBoundary { offset: 2 })
        ));
        assert!(matches!(
            apply_edits(
                "abc",
                vec![Edit {
                    start: 4,
                    end: 4,
                    replacement: String::new(),
                }]
            ),
            Err(EditError::InvalidRange { .. })
        ));
    }
}
