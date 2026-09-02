from src.classify import classify, label


def test_negative():
    assert classify(-1, "strict") == "negative"


def test_label():
    assert label(1) == "some"
    assert label(0) == "none"
