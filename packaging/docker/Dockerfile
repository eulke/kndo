# kndo container image. Never builds kndo — copies the already-built release binary in,
# per-arch, matching every other secondary distribution channel's "the GitHub Release is
# the source of truth" rule.
#
# Built by CI as: docker buildx build --platform linux/amd64,linux/arm64
#                   --build-context bin=<dir with kndo-linux-amd64 / kndo-linux-arm64>
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
COPY --from=bin kndo-linux-${TARGETARCH} /usr/local/bin/kndo
RUN chmod +x /usr/local/bin/kndo

ENTRYPOINT ["/usr/local/bin/kndo"]
CMD ["check"]
