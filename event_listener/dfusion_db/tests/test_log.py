from django.test import TestCase
from django_eth_events.event_listener import EventListener


class TestLog(TestCase):
    def test_console_event(self):
        event_listener = EventListener()
        event_listener.execute()


