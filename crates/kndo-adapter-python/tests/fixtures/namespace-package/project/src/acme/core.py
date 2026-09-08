from acme import extra


def run():
    """`acme` has no `__init__.py` anywhere: it is a PEP 420 namespace whose
    portions live under two declared source roots."""
    return extra._shared()
