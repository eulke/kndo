def pytest_configure(config):
    """pytest imports this module because the manifest registered it under the
    `pytest11` group. No line of this project names it."""
    config.addinivalue_line("markers", "slow")
