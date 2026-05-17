---
name: python
trigger: ["python", "django", "fastapi", "flask", "pytest", "pip", "poetry", "pyproject", "pydantic", ".py", "def ", "class "]
tokens: ~550
---

## Python conventions

**Style:** Follow PEP 8. Use `black` for formatting, `ruff` for linting. Line length 88 (black default). Double quotes for strings.

**Types:** Use type hints everywhere — function signatures, class attributes, return types. Use `from __future__ import annotations` for forward references. Use `Optional[X]` or `X | None` (Python 3.10+).

**Imports:** Standard library first, then third-party, then local. Use absolute imports. Never `from module import *`.

**Classes:** Prefer `dataclasses` or `pydantic` models over raw `__init__` methods for data classes. Use `@property` sparingly — a plain attribute is better than a trivial property.

**Error handling:** Create project-specific exception classes. Catch specific exceptions, not bare `except:`. Use context managers (`with`) for resource management.

**Async:** Use `asyncio` with `async def` / `await`. Don't mix sync and async code — pick one per layer. Use `httpx` for async HTTP, `aiofiles` for async file I/O.

**Testing:** Use `pytest`. Tests in `tests/` mirroring the source tree. Use `pytest.fixture` for setup. Prefer `pytest-asyncio` for async tests. Mock with `unittest.mock.patch` or `pytest-mock`.

**Dependencies:** Pin in `pyproject.toml` (poetry) or `requirements.txt`. Use virtual environments. Don't install globally.

**Logging:** Use `logging` module, not `print()`. Configure at the entry point, not in libraries.
