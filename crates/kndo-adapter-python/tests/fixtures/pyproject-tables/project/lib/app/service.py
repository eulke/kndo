import requests


def serve():
    """Re-exported by the package door, which the manifest makes an entry."""
    return requests.get("https://example.invalid").status_code


def dangling():
    """Public, and named by nothing. In an uploadable distribution the export
    surface would keep it; this one says `Private :: Do Not Upload`."""
    return 0
