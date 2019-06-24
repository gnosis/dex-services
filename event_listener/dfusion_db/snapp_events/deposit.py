import logging
from typing import Dict, Any

from event_listener.dfusion_db.snapp_event_receiver import SnappEventListener
from ..models import Deposit

class DepositReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Deposit.from_dictionary(event))

    def save_parsed(self, deposit: Deposit) -> None:
        self.database.write_deposit(deposit)
