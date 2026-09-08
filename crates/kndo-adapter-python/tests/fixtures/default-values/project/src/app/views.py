from .base import Model


def _clamp(n):
    return max(0, n)


def _unclamped(n):
    """The same shape, defaulted by nothing and called by nobody."""
    return n


def index(limit: int = 30, guard=_clamp, typed_guard: object = _clamp):
    """`guard=_clamp` and `typed_guard: object = _clamp` each bind one name and
    READ another. A binder seat named by parent KIND swallowed the default."""
    return Model(), guard(limit), typed_guard(limit)


class Widget(Model, metaclass=type):
    pass
