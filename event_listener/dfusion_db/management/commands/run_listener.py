from django.core.management.base import BaseCommand

from django_eth_events.event_listener import EventListener


class Command(BaseCommand):
    help = 'Some sort of help message'

    def handle(self, *args, **options):
        print("Beginning event listener")
        event_listener = EventListener()
        while 1:
            event_listener.execute()

        self.stdout.write("Infinite Loop finished", ending='')
