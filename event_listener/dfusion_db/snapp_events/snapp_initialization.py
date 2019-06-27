import logging
from typing import Dict, Any

from event_listener.dfusion_db.snapp_event_receiver import SnappEventListener
from ..models import AccountRecord


class SnappInitializationReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:

        # Verify integrity of post data
        assert event.keys() == {'stateHash', 'maxTokens', 'maxAccounts'}, "Unexpected Event Keys"
        state_hash = event['stateHash']
        assert isinstance(state_hash, str) and len(state_hash) == 64, "StateHash has unexpected value %s" % state_hash
        assert isinstance(event['maxTokens'], int), "maxTokens has unexpected value"
        assert isinstance(event['maxAccounts'], int), "maxAccounts has unexpected value"

        self.initialize_accounts(event['maxTokens'], event['maxAccounts'], state_hash)

    def initialize_accounts(self, num_tokens: int, num_accounts: int, state_hash: str) -> None:
        account_record = AccountRecord(0, state_hash, [0 for _ in range(num_tokens * num_accounts)])
        self.database.write_snapp_constants(num_tokens, num_accounts)
        self.database.write_account_state(account_record)
        logging.info("Successfully included Snapp Initialization constants and account record")


class AuctionInitializationReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:

        # Verify integrity of post data
        assert event.keys() == {'maxOrders', 'numReservedAccounts', 'ordersPerReservedAccount'}, "Unexpected Event Keys"
        assert isinstance(event['maxOrders'], int), "maxOrders has unexpected value"
        assert isinstance(event['numReservedAccounts'], int), "numReservedAccounts has unexpected value"
        assert isinstance(event['ordersPerReservedAccount'], int), "ordersPerReservedAccount has unexpected value"

        self.database.write_auction_constants(
            event['maxOrders'], event['numReservedAccounts'], event['ordersPerReservedAccount']
        )
        logging.info("Successfully included Snapp Auction constant(s)")
