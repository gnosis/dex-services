FROM gnosispm/dfusion-event-listener-base

ENV PYTHONUNBUFFERED 1
WORKDIR /app

# Restore cache if available (at least one sourcefile is needed, therefore use README in case cache doesn't exist)
ADD README.md .cache/pip_cache.tar.gz* /usr/local/lib/python3.6/

COPY event_listener/requirements.txt event_listener/
RUN pip install --no-cache-dir -r event_listener/requirements.txt 

ENV PYTHONPATH=/app/