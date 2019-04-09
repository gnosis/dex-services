import logging
from abc import ABC, abstractmethod
from typing import Dict, Any, Union, List, Optional

from .database_interface import DatabaseInterface, MongoDbInterface
from .models import Deposit, StateTransition, TransitionType, Withdraw, AccountRecord, Order


class SnappEventListener(ABC):
    """Abstract SnappEventReceiver class."""

    def __init__(self, database_interface: Optional[DatabaseInterface] = None):
        self.database = database_interface if database_interface else MongoDbInterface()
        self.logger = logging.getLogger(__name__)

    @abstractmethod
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        pass


class DepositReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Deposit.from_dictionary(event))

    def save_parsed(self, deposit: Deposit) -> None:
        try:
            self.database.write_deposit(deposit)
        except AssertionError as exc:
            logging.critical(
                "Failed to record Deposit [{}] - {}".format(exc, deposit))


class StateTransitionReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(StateTransition.from_dictionary(event))

    def save_parsed(self, transition: StateTransition) -> None:
        try:
            self.__update_accounts(transition)
            logging.info("Successfully updated state and balances")
        except AssertionError as exc:
            logging.critical(
                "Failed to record StateTransition [{}] - {}".format(exc, transition))

    def __update_accounts(self, transition: StateTransition) -> None:
        balances = self.database.get_account_state(transition.state_index - 1).balances.copy()
        num_tokens = self.database.get_num_tokens()
        for datum in self.__get_data_to_apply(transition):
            # Balances are stored as [b(a1, t1), b(a1, t2), ... b(a1, T), b(a2, t1), ...]
            index = num_tokens * (datum.account_id - 1) + (datum.token_id - 1)

            if transition.transition_type == TransitionType.Deposit:
                self.logger.info("Incrementing balance of account {} - token {} by {}".format(
                    datum.account_id, datum.token_id, datum.amount))
                balances[index] += datum.amount
            elif transition.transition_type == TransitionType.Withdraw:
                assert isinstance(datum, Withdraw)
                if balances[index] - datum.amount >= 0:
                    self.logger.info(
                        "Decreasing balance of account {} - token {} by {}".format(
                            datum.account_id,
                            datum.token_id,
                            datum.amount
                        )
                    )
                    balances[index] -= datum.amount
                    self.database.update_withdraw(datum, datum._replace(valid=True))
                else:
                    self.logger.info(
                        "Insufficient balance: account {} - token {} for amount {}".format(
                            datum.account_id,
                            datum.token_id,
                            datum.amount
                        )
                    )
            else:
                # This can not happen
                self.logger.error("Unrecognized transition type - this should never happen")

        new_account_record = AccountRecord(transition.state_index, transition.state_hash, balances)
        self.database.write_account_state(new_account_record)

    def __get_data_to_apply(self, transition: StateTransition) -> Union[List[Withdraw], List[Deposit]]:
        if transition.transition_type == TransitionType.Deposit:
            return self.database.get_deposits(transition.slot)
        elif transition.transition_type == TransitionType.Withdraw:
            return self.database.get_withdraws(transition.slot)
        else:
            raise Exception("Invalid transition type: {} ".format(transition.transition_type))


class SnappInitializationReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:

        # Verify integrity of post data
        assert event.keys() == {'stateHash', 'maxTokens', 'maxAccounts'}, "Unexpected Event Keys"
        state_hash = event['stateHash']
        assert isinstance(state_hash, str) and len(state_hash) == 64, "StateHash has unexpected values %s" % state_hash
        assert isinstance(event['maxTokens'], int), "maxTokens has unexpected values"
        assert isinstance(event['maxAccounts'], int), "maxAccounts has unexpected values"

        try:
            self.initialize_accounts(event['maxTokens'], event['maxAccounts'], state_hash)
        except AssertionError as exc:
            logging.critical(
                "Failed to record SnappInitialization [{}] - {}".format(exc, event))

    def initialize_accounts(self, num_tokens: int, num_accounts: int, state_hash: str) -> None:
        account_record = AccountRecord(0, state_hash, [0 for _ in range(num_tokens * num_accounts)])
        self.database.write_constants(num_tokens, num_accounts)
        self.database.write_account_state(account_record)


class WithdrawRequestReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Withdraw.from_dictionary(event))

    def save_parsed(self, withdraw: Withdraw) -> None:
        try:
            self.database.write_withdraw(withdraw)
        except AssertionError as exc:
            logging.critical(
                "Failed to record Deposit [{}] - {}".format(exc, withdraw))


class OrderReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(Order.from_dictionary(event))

    def save_parsed(self, order: Order) -> None:
        try:
            self.database.write_order(order)
        except AssertionError as exc:
            logging.critical(
                "Failed to record Deposit [{}] - {}".format(exc, order))
