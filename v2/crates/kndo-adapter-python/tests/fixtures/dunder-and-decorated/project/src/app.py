def route(f):
    return f


class Money:
    # The runtime protocol calls this (`a == b`) — no source line ever will.
    def __eq__(self, other):
        return True


@route
def handler():
    # `@d def f` IS `f = d(f)`: handed to its decorator by the language.
    return Money()


def _forgotten():
    # Module-private by convention and referenced by nothing: the one rung
    # Python can bound, judged. A PUBLIC dead def stays published surface.
    return 0
