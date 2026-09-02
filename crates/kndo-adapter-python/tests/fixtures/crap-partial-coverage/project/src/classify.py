def classify(value, mode):
    if value < 0:
        return "negative"
    if value == 0:
        return "zero"
    if value > 100 and mode == "strict":
        return "huge"
    if value > 50 or mode == "loose":
        return "large"
    if mode == "strict":
        return "strict-small"
    if value % 2 == 0:
        return "even"
    if value % 3 == 0:
        return "triple"
    return "odd"


def label(points):
    if points > 0:
        return "some"
    return "none"
