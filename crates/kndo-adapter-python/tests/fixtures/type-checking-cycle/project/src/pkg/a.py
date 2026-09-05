# An import under `if TYPE_CHECKING:` is False at run time: b.py importing this
# module back closes no initialization loop, so neither module is cyclic.
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .b import B


def make() -> "B":
    from .b import B  # a function-scoped import runs later, never at load

    return B()
