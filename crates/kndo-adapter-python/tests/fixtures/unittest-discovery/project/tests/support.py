"""Shared helpers for the suite. Under `testpaths`, and named by nothing:
no test module imports it, and its name matches no runner pattern."""


def make_calc_case(base):
    return {"base": base}


def _scratch(value):
    return value
