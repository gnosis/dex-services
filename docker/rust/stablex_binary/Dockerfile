FROM python:3.6-slim-stretch as build-image
RUN apt-get update \ 
  &&  apt-get install -y --no-install-recommends libpq5 libncurses5 \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
ENV PATH="/opt/venv/bin:$PATH"
ENV PYTHONPATH ${PYTHONPATH}:/app/batchauctions/

# COPY --from=stablex /opt/venv /opt/venv
# COPY --from=stablex /app/batchauctions batchauctions/
# COPY --from=stablex /usr/local/bin/scipampl /usr/local/bin/

COPY target/debug/stablex stablex

COPY target/debug/listener graph-listener
COPY target/debug/dfusion dfusion