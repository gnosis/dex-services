FROM python:3.6-slim-stretch

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential libssl-dev \
    && rm -rf /var/lib/apt/lists/*

ENV PYTHONUNBUFFERED 1
WORKDIR /app
ENV PYTHONPATH=/app/

COPY event_listener/requirements.txt event_listener/
RUN pip install --no-cache-dir -r event_listener/requirements.txt 