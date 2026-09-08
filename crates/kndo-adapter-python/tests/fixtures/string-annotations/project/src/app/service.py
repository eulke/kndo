from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .models import _Later


def resolve(target: "_Later") -> "_Later | None":
    """The quotes are there because the name is not bound at runtime, not
    because it is a string: `_Later` is named here exactly as unquoted."""
    return target
