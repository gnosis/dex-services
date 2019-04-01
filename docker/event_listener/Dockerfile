FROM python:3.6-slim-stretch

ENV PYTHONUNBUFFERED 1
WORKDIR /app

# Restore cache if available (at least one sourcefile is needed, therefore use README in case cache doesn't exist)
ADD README.md .cache/pip_cache.tar.gz* /usr/local/lib/python3.6/

COPY event_listener/requirements.txt event_listener/
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --no-cache-dir -r event_listener/requirements.txt 

ENV PYTHONPATH=/app/