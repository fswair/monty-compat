from os import PathLike
from typing import Literal, final

@final
class TranspilationError(Exception):
    """Source cannot be lowered safely for the requested Monty release."""

def transpiler(
    code: str,
    release: str | Literal["verified", "latest"] = "verified",
) -> str:
    """Return source lowered for a verified or exact Monty release.

    ``verified`` uses the wheel's embedded manifest. ``latest`` resolves the
    newest compatible published manifest. A bare bundled version or a
    ``v``-prefixed tag selects an exact release. Resolution failures and
    non-representable semantics raise :class:`TranspilationError`; the function
    never executes ``code``.
    """

@final
class ExtractionError(Exception): ...

def _extract_local_json(root: str | PathLike[str]) -> str: ...
def _extract_archive_json(archive: bytes) -> str: ...
def _extract_github_json(url: str, only_released: bool) -> str: ...
