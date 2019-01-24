FROM python:3.6-slim-stretch

ENV PYTHONUNBUFFERED 1
WORKDIR /app

COPY . .
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential libssl-dev \
    && pip install --no-cache-dir -r event_listener/requirements.txt 

ENV PYTHONPATH=/app/