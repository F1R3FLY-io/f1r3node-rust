FROM ghcr.io/oracle/oci-cli@sha256:12ba572de6290354255e9d7ed0d387a428230a0a5b2b8969603ca8008f71734a
USER root
COPY --chmod=755 casper-campaign-supervisor /function/casper-campaign-supervisor
COPY --chmod=755 start.sh /function/start.sh
USER oracle
ENV OCI_CLI_AUTH=resource_principal
ENTRYPOINT ["/function/start.sh"]
