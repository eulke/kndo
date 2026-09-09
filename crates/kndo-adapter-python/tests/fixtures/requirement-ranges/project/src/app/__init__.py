from werkzeug.wrappers import Response


def serve() -> Response:
    return Response("ok")
