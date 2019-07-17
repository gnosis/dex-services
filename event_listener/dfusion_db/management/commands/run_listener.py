from django.core.management.base import BaseCommand
from django_eth_events.event_listener import EventListener
from typing import Any
import time

import logging

_log = logging.getLogger(__name__)


class Command(BaseCommand):  # type: ignore
    def handle(self, *args: Any, **options: Any) -> None:
        _log.info("Event Listener now active")
        event_listener = EventListener()
        while 1:
            event_listener.execute()
            time.sleep(2)
