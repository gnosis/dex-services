import logging
from typing import Dict, Any

from event_listener.dfusion_db.snapp_event_receiver import SnappEventListener
from ..models import AuctionSettlement, AccountRecord


class AuctionSettlementReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(
            AuctionSettlement.from_dictionary(
                event,
                self.database.get_num_tokens(),
                self.database.get_num_orders()
            )
        )

    def save_parsed(self, settlement: AuctionSettlement) -> None:
        self.__update_accounts(settlement)

    def __update_accounts(self, settlement: AuctionSettlement) -> None:
        state = self.database.get_account_state(settlement.state_index - 1)
        balances = state.balances.copy()

        orders = self.database.get_orders(settlement.auction_id)
        num_tokens = self.database.get_num_tokens()
        solution = settlement.prices_and_volumes

        buy_amounts = solution.buy_amounts
        sell_amounts = solution.sell_amounts

        for i, order in enumerate(orders):
            buy_index = num_tokens * order.account_id + order.buy_token
            balances[buy_index] += buy_amounts[i]

            sell_index = num_tokens * order.account_id + order.sell_token
            balances[sell_index] -= sell_amounts[i]

        new_account_record = AccountRecord(settlement.state_index, settlement.state_hash, balances)
        self.database.write_account_state(new_account_record)
