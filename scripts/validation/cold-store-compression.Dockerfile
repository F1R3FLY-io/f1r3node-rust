ARG BASE
FROM ${BASE}
USER root
COPY --chmod=755 node /opt/docker/bin/node
COPY --chmod=755 docker-entrypoint.sh /opt/docker/bin/docker-entrypoint.sh
COPY runtime-libs/ /opt/validation-libs/
RUN chmod -R a+rX /opt/validation-libs
ENV LD_LIBRARY_PATH=/opt/validation-libs
USER daemon
