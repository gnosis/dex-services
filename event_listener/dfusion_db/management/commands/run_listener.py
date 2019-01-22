from django.core.management.base import BaseCommand
from django_eth_events.event_listener import EventListener
import time

import logging

_log = logging.getLogger(__name__)


class Command(BaseCommand):

    def handle(self, *args, **options):
        _log.info("Event Listener now active")
        event_listener = EventListener()
        while 1:
            event_listener.execute()
            time.sleep(3)
