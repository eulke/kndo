import unittest

from app.calc import add


class CalcTest(unittest.TestCase):
    def setUp(self):
        """unittest calls this itself — a member the source never names."""
        self.base = 1

    def test_add(self):
        """The runner dispatches this by NAME: nothing in the tree calls it."""
        self.assertEqual(add(self.base, 2), 3)

    def _not_a_test(self):
        """The control: a member of the same class the runner never collects
        and nothing else names."""
        return None
