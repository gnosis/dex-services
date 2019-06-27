import logging
from typing import Dict, Any

from ..snapp_event_receiver import SnappEventListener
from ..models import Withdraw


class WithdrawRequestReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Withdraw.from_dictionary(event))

    def save_parsed(self, withdraw: Withdraw) -> None:
        self.database.write_withdraw(withdraw)
