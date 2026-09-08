# The stdlib module this package SHADOWS by name — imported absolutely, from
# inside the package that shares its name.
import json


def dumps(value):
    return json.dumps(value)


def _unused_helper():
    """Named by nothing: the control that keeps the fixture from passing by
    keeping the module alive wholesale."""
    return None
