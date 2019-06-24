import logging
from typing import Dict, Any, Union, List

from event_listener.dfusion_db.snapp_event_receiver import SnappEventListener
from ..models import Deposit, StateTransition, TransitionType, Withdraw, AccountRecord

class StateTransitionReceiver(SnappEventListener):
    def save(self, event: Dict[str, Any], block_info: Dict[str, Any]) -> None:
        self.save_parsed(StateTransition.from_dictionary(event))

    def save_parsed(self, transition: StateTransition) -> None:
        self.__update_accounts(transition)
        logging.info("Successfully updated state and balances")

    def __update_accounts(self, transition: StateTransition) -> None:
        balances = self.database.get_account_state(transition.state_index - 1).balances.copy()
        num_tokens = self.database.get_num_tokens()
        for datum in self.__get_data_to_apply(transition):
            # Balances are stored as [b(a1, t1), b(a1, t2), ... b(a1, T), b(a2, t1), ...]
            index = num_tokens * datum.account_id + datum.token_id

            if transition.transition_type == TransitionType.Deposit:
                self.logger.info(
                    "Incrementing balance of account {} - token {} by {}".format(
                        datum.account_id,
                        datum.token_id,
                        datum.amount
                    )
                )
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
                self.logger.error("Unrecognized transition type: should never happen!")

        new_account_record = AccountRecord(transition.state_index, transition.state_hash, balances)
        self.database.write_account_state(new_account_record)

    def __get_data_to_apply(self, transition: StateTransition) -> Union[List[Withdraw], List[Deposit]]:
        if transition.transition_type == TransitionType.Deposit:
            return self.database.get_deposits(transition.slot)
        elif transition.transition_type == TransitionType.Withdraw:
            return self.database.get_withdraws(transition.slot)
        else:
            raise Exception("Invalid transition type: {} ".format(transition.transition_type))

