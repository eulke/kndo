def _qualified():
    """Named only as `inner._qualified()` from a sibling module — the QUALIFIED
    form, which the receiver evidence resolves to this module."""
    return 1


def _alone():
    """The same shape, named by nobody: module-private really does mean the
    module, and this is the accusation that proves it."""
    return 2
