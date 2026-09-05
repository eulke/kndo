# The control: a module-level import in both directions IS an initialization
# cycle, anchored at the lexicographically first participant (this module).
from .d import d


def c():
    return d()
