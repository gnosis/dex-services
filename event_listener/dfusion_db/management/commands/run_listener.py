from django.core.management.base import BaseCommand
from django_eth_events.event_listener import EventListener


class Command(BaseCommand):

    def handle(self, *args, **options):
        event_listener = EventListener()
        while 1:
            event_listener.execute()
