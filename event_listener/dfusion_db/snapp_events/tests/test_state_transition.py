import unittest
from unittest.mock import Mock

from event_listener.dfusion_db.models import AccountRecord, Deposit, TransitionType, Withdraw, StateTransition
from .constants import EMPTY_STATE_HASH
from ..state_transition import StateTransitionReceiver


class StateTransitionReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        new_state_index = 2
        slot_index = 3
        num_tokens = 10

        database.get_num_tokens.return_value = num_tokens
        old_state = AccountRecord(1, "old state", [42] * 10 * num_tokens)
        database.get_account_state.return_value = old_state

        deposit1 = Deposit(0, 1, 10, slot_index, 0)
        deposit2 = Deposit(6, 2, 5, slot_index, 1)
        database.get_deposits.return_value = [deposit1, deposit2]

        transition_event = {
            "transitionType": TransitionType.Deposit,
            "stateIndex": new_state_index,
            "stateHash": EMPTY_STATE_HASH,
            "slot": slot_index,
        }
        receiver.save(event=transition_event, block_info={})

        new_balances = old_state.balances
        new_balances[1] = 52
        new_balances[62] = 47
        new_state = AccountRecord(
            new_state_index, EMPTY_STATE_HASH, new_balances)
        database.write_account_state.assert_called_with(new_state)

    @staticmethod
    def test_adds_pending_deposits_to_previous_balances() -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        new_state_index = 2
        slot_index = 3
        num_tokens = 10

        database.get_num_tokens.return_value = num_tokens
        old_state = AccountRecord(1, "old state", [42] * 10 * num_tokens)
        database.get_account_state.return_value = old_state

        deposit1 = Deposit(0, 1, 10, slot_index, 0)
        deposit2 = Deposit(6, 2, 5, slot_index, 1)
        database.get_deposits.return_value = [deposit1, deposit2]

        transition = StateTransition(
            TransitionType.Deposit, new_state_index, "new state", slot_index)
        receiver.save_parsed(transition)

        new_balances = old_state.balances
        new_balances[1] = 52
        new_balances[62] = 47
        new_state = AccountRecord(new_state_index, "new state", new_balances)
        database.write_account_state.assert_called_with(new_state)

    @staticmethod
    def test_subtracts_pending_withdraws_to_previous_balances() -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        new_state_index = 2
        slot_index = 3
        num_tokens = 10

        database.get_num_tokens.return_value = num_tokens
        old_state = AccountRecord(1, "old state", [42] * 10 * num_tokens)
        database.get_account_state.return_value = old_state

        withdraw1 = Withdraw(0, 1, 10, slot_index, 0)
        withdraw2 = Withdraw(6, 2, 5, slot_index, 1)
        database.get_withdraws.return_value = [withdraw1, withdraw2]

        transition = StateTransition(
            TransitionType.Withdraw, new_state_index, "new state", slot_index)
        receiver.save_parsed(transition)

        new_balances = old_state.balances
        new_balances[1] = 32
        new_balances[62] = 37
        new_state = AccountRecord(new_state_index, "new state", new_balances)
        database.write_account_state.assert_called_with(new_state)

    @staticmethod
    def test_marks_valid_withdraws_as_valid() -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        new_state_index = 2
        slot_index = 3
        num_tokens = 10

        database.get_num_tokens.return_value = num_tokens
        old_state = AccountRecord(1, "old state", [42] * 10 * num_tokens)
        database.get_account_state.return_value = old_state

        withdraw1 = Withdraw(0, 1, 10, slot_index, 0)
        withdraw2 = Withdraw(6, 2, 100, slot_index, 1)
        database.get_withdraws.return_value = [withdraw1, withdraw2]

        transition = StateTransition(
            TransitionType.Withdraw, new_state_index, "new state", slot_index)
        receiver.save_parsed(transition)

        updated_withdraw1 = withdraw1._replace(valid=True)
        database.update_withdraw.assert_called_once_with(withdraw1, updated_withdraw1)

    @staticmethod
    def test_skips_deduction_if_not_enough_balance() -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        new_state_index = 2
        slot_index = 3
        num_tokens = 10

        database.get_num_tokens.return_value = num_tokens
        old_state = AccountRecord(1, "old state", [42] * 10 * num_tokens)
        database.get_account_state.return_value = old_state

        withdraw1 = Withdraw(0, 1, 10, slot_index, 0)
        withdraw2 = Withdraw(6, 2, 100, slot_index, 1)
        database.get_withdraws.return_value = [withdraw1, withdraw2]

        transition = StateTransition(
            TransitionType.Withdraw, new_state_index, "new state", slot_index)
        receiver.save_parsed(transition)

        new_balances = old_state.balances
        new_balances[1] = 32
        new_state = AccountRecord(new_state_index, "new state", new_balances)
        database.write_account_state.assert_called_with(new_state)

    def test_raises_on_bad_transition_type(self) -> None:
        database = Mock()
        receiver = StateTransitionReceiver(database)
        transition_event = {
            "transitionType": -1,
            "stateIndex": 2,
            "stateHash": "new state",
            "slot": 3,
        }
        with self.assertRaises(Exception):
            receiver.save(event=transition_event, block_info={})
