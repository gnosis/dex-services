import unittest
from unittest.mock import Mock
from typing import List
from ..snapp_event_receiver import DepositReceiver, OrderReceiver, SnappInitializationReceiver, \
    WithdrawRequestReceiver, StateTransitionReceiver, AuctionSettlementReceiver, AuctionInitializationReceiver, \
    StandingOrderBatchReceiver
from ..models import Deposit, StateTransition, TransitionType, Withdraw, AccountRecord, Order, StandingOrder

EMPTY_STATE_HASH = "0x00000000000000000000000000000000000000000000000000000000000000"


class DepositReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save_parsed() -> None:
        database = Mock()
        receiver = DepositReceiver(database)
        deposit = Deposit(1, 2, 10, 42, 51)
        receiver.save_parsed(deposit)
        database.write_deposit.assert_called_with(deposit)

    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = DepositReceiver(database)
        event = {
            "accountId": 1,
            "tokenId": 2,
            "amount": 3,
            "slot": 4,
            "slotIndex": 5
        }
        receiver.save(event, block_info={})
        database.write_deposit.assert_called_with(Deposit.from_dictionary(event))


class WithdrawRequestReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = WithdrawRequestReceiver(database)
        event = {
            "accountId": 1,
            "tokenId": 2,
            "amount": "3",
            "slot": 4,
            "slotIndex": 5,
        }
        receiver.save(event, block_info={})
        database.write_withdraw.assert_called_with(Withdraw.from_dictionary(event))

    @staticmethod
    def test_writes_withdraw() -> None:
        database = Mock()
        receiver = WithdrawRequestReceiver(database)
        withdraw = Withdraw(1, 2, 10, 42, 51)
        receiver.save_parsed(withdraw)
        database.write_withdraw.assert_called_with(withdraw)


class OrderReceiverTest(unittest.TestCase):
    @staticmethod
    def test_save() -> None:
        database = Mock()
        receiver = OrderReceiver(database)
        event = {
            "auctionId": 1,
            "slotIndex": 2,
            "accountId": 3,
            "buyToken": 4,
            "sellToken": 5,
            "buyAmount": "67",
            "sellAmount": "89",
        }
        receiver.save(event, block_info={})
        database.write_order.assert_called_with(Order.from_dictionary(event))

    @staticmethod
    def test_writes_order() -> None:
        database = Mock()
        receiver = OrderReceiver(database)
        order = Order(1, 1, 2, 1, 1, 1, 1)
        receiver.save_parsed(order)
        database.write_order.assert_called_with(order)


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
            "stateHash": "0x00000000000000000000000000000000000000000000000000000000000000",
            "slot": slot_index,
        }
        receiver.save(event=transition_event, block_info={})

        new_balances = old_state.balances
        new_balances[1] = 52
        new_balances[62] = 47
        new_state = AccountRecord(
            new_state_index, "0x00000000000000000000000000000000000000000000000000000000000000", new_balances)
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


class SnappInitializationReceiverTest(unittest.TestCase):

    @staticmethod
    def test_generic_save() -> None:
        database = Mock()
        receiver = SnappInitializationReceiver(database)

        num_tokens = 2
        num_accounts = 3

        event = {
            "stateHash": EMPTY_STATE_HASH,
            "maxTokens": num_tokens,
            "maxAccounts": num_accounts
        }
        receiver.save(event, block_info={})

        database.write_snapp_constants.assert_called_with(2, 3)
        database.write_account_state.assert_called_with(
            AccountRecord(0, EMPTY_STATE_HASH, [0 for _ in range(num_tokens * num_accounts)]))

    @staticmethod
    def test_create_empty_balances() -> None:
        database = Mock()
        receiver = SnappInitializationReceiver(database)

        receiver.initialize_accounts(30, 100, "initial hash")

        state = AccountRecord(0, "initial hash", [0] * 30 * 100)
        database.write_account_state.assert_called_with(state)
        database.write_snapp_constants.assert_called_with(30, 100)


class AuctionSettlementReceiverTest(unittest.TestCase):

    @staticmethod
    def test_save() -> None:
        num_tokens = 2
        num_accounts = 2
        num_orders = 2
        old_balances = [42] * num_accounts * num_tokens
        dummy_account_record = AccountRecord(1, EMPTY_STATE_HASH, old_balances)

        def int_list_to_hex_bytes(arr: List[int], num_bits: int) -> str:
            assert num_bits % 4 == 0
            hex_length = num_bits // 4
            return "".join(map(lambda t: str(hex(t))[2:].rjust(hex_length, "0"), arr))

        database = Mock()
        receiver = AuctionSettlementReceiver(database)

        orders = [
            Order.from_dictionary({
                "auctionId": 1,
                "slotIndex": 4,
                "accountId": 0,
                "buyToken": 1,
                "sellToken": 0,
                "buyAmount": 10,
                "sellAmount": 10,
            }),
            Order.from_dictionary({
                "auctionId": 1,
                "slotIndex": 4,
                "accountId": 1,
                "buyToken": 0,
                "sellToken": 1,
                "buyAmount": 8,
                "sellAmount": 16,
            }),
        ]

        prices = [16, 10]
        executed_amounts = [16, 10, 10, 16]
        encoded_solution = int_list_to_hex_bytes(prices, 96) + int_list_to_hex_bytes(executed_amounts, 96)

        event = {
            "auctionId": 1,
            "stateIndex": 2,
            "stateHash": EMPTY_STATE_HASH,
            "pricesAndVolumes": encoded_solution
        }

        database.get_account_state.return_value = dummy_account_record
        database.get_orders.return_value = orders
        database.get_num_tokens.return_value = num_tokens
        database.get_num_orders.return_value = num_orders
        receiver.save(event, block_info={})

        new_balances = [42 - 10, 42 + 16, 42 + 10, 42 - 16]

        new_account_record = AccountRecord(2, EMPTY_STATE_HASH, new_balances)
        database.write_account_state.assert_called_with(new_account_record)


class AuctionInitializationReceiverTest(unittest.TestCase):

    @staticmethod
    def test_generic_save() -> None:
        database = Mock()
        receiver = AuctionInitializationReceiver(database)

        event = {"maxOrders": 2, 'numReservedAccounts': 1, 'ordersPerReservedAccount': 1}
        receiver.save(event, block_info={})

        database.write_auction_constants.assert_called_with(2, 1, 1)

    def test_unexpected_save(self) -> None:
        database = Mock()
        receiver = AuctionInitializationReceiver(database)

        # Bad Value
        with self.assertRaises(AssertionError):
            receiver.save({"maxOrders": "not an integer"}, block_info={})

        # Bad Key
        with self.assertRaises(AssertionError):
            receiver.save({"badKey": 1}, block_info={})


class StandingOrderBatchReceiverTest(unittest.TestCase):

    @staticmethod
    def test_writes_order() -> None:
        database = Mock()
        receiver = StandingOrderBatchReceiver(database)
        order = StandingOrder(1, 2, 3, [])
        receiver.save_parsed(order)
        database.write_standing_order.assert_called_with(order)
