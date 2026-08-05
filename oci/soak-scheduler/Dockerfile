FROM ghcr.io/oracle/oci-cli@sha256:12ba572de6290354255e9d7ed0d387a428230a0a5b2b8969603ca8008f71734a
USER root
RUN dnf install -y bash curl jq openssl tzdata \
    && dnf clean all
COPY --from=fnproject/hotwrap@sha256:bf6303d7d216581c0e760f33dd74c3cdea83edad69f3d9614b7f573ba62c22b4 /hotwrap /hotwrap
WORKDIR /function
COPY --chown=oracle:oracle handler.sh ./
RUN chmod 755 /function/handler.sh /hotwrap
USER oracle
CMD ["/function/handler.sh"]
ENTRYPOINT ["/hotwrap"]
