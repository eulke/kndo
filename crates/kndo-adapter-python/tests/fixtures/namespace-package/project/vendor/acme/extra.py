def _shared():
    """Named as `extra._shared()` from the other portion of `acme` — a
    qualified reference whose receiver is a sibling module of this one."""
    return 1


def _alone():
    """The same shape, named by nobody."""
    return 2
