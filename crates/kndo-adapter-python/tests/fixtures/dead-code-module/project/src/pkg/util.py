def live():
    return _shared()


def dead():
    return 1


def _shared():
    return 2


def _ghost():
    return 3
