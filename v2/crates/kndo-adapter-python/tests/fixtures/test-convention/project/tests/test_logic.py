from src.logic import add


def test_add(client):
    assert add(1, 2) == 3
