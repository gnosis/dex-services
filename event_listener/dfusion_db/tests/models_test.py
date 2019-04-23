import unittest
from ..models import AccountRecord, AuctionResults, AuctionSettlement
from ..models import Deposit, Order, StateTransition, TransitionType, Withdraw


class DepositTest(unittest.TestCase):
    def test_from_dictionary(self) -> None:
        deposit = Deposit.from_dictionary({
            "accountId": 1,
            "tokenId": 2,
            "amount": 3,
            "slot": 4,
            "slotIndex": 5
        })
        self.assertEqual(1, deposit.account_id)
        self.assertEqual(2, deposit.token_id)
        self.assertEqual(3, deposit.amount)
        self.assertEqual(4, deposit.slot)
        self.assertEqual(5, deposit.slot_index)

    def test_to_dictionary(self) -> None:
        deposit = Deposit(1, 2, 3, 4, 5)
        expected = {
            "accountId": 1,
            "tokenId": 2,
            "amount": "3",
            "slot": 4,
            "slotIndex": 5
        }
        self.assertEqual(deposit.to_dictionary(), expected)

    def test_throws_with_missing_key(self) -> None:
        with self.assertRaises(AssertionError):
            Deposit.from_dictionary({
                "accountId": 1,
                "tokenId": 2,
                "amount": 3,
                "slot": 4,
            })

    def test_throws_with_non_integer_value(self) -> None:
        with self.assertRaises(ValueError):
            Deposit.from_dictionary({
                "accountId": 1,
                "tokenId": 2,
                "amount": 3,
                "slot": 4,
                "slotIndex": "foo"
            })


class WithdrawTest(unittest.TestCase):
    def test_from_dictionary(self) -> None:
        withdraw = Withdraw.from_dictionary({
            "accountId": 1,
            "tokenId": 2,
            "amount": 3,
            "slot": 4,
            "slotIndex": 5
        })
        self.assertEqual(1, withdraw.account_id)
        self.assertEqual(2, withdraw.token_id)
        self.assertEqual(3, withdraw.amount)
        self.assertEqual(4, withdraw.slot)
        self.assertEqual(5, withdraw.slot_index)

    def test_to_dictionary(self) -> None:
        withdraw = Withdraw(1, 2, 3, 4, 5)
        expected = {
            "accountId": 1,
            "tokenId": 2,
            "amount": "3",
            "slot": 4,
            "slotIndex": 5,
            "valid": False
        }
        self.assertEqual(withdraw.to_dictionary(), expected)

    def test_throws_with_missing_key(self) -> None:
        with self.assertRaises(AssertionError):
            Withdraw.from_dictionary({
                "accountId": 1,
                "tokenId": 2,
                "amount": 3,
                "slot": 4,
            })

    def test_throws_with_non_integer_value(self) -> None:
        with self.assertRaises(ValueError):
            Withdraw.from_dictionary({
                "accountId": 1,
                "tokenId": 2,
                "amount": 3,
                "slot": 4,
                "slotIndex": "foo"
            })


class OrderTest(unittest.TestCase):
    def test_from_dictionary(self) -> None:
        order = Order.from_dictionary({
            "auctionId": 1,
            "slotIndex": 2,
            "accountId": 3,
            "buyToken": 4,
            "sellToken": 5,
            "buyAmount": 6,
            "sellAmount": 7
        })

        self.assertEqual(1, order.slot)
        self.assertEqual(2, order.slot_index)
        self.assertEqual(3, order.account_id)
        self.assertEqual(4, order.buy_token)
        self.assertEqual(5, order.sell_token)
        self.assertEqual(6, order.buy_amount)
        self.assertEqual(7, order.sell_amount)

    def test_to_dictionary(self) -> None:
        order = Order(1, 2, 3, 4, 5, 6, 7)
        expected = {
            "auctionId": 1,
            "slotIndex": 2,
            "accountId": 3,
            "buyToken": 4,
            "sellToken": 5,
            "buyAmount": "6",
            "sellAmount": "7",
        }
        self.assertEqual(order.to_dictionary(), expected)

    def test_throws_with_missing_key(self) -> None:
        with self.assertRaises(AssertionError):
            Order.from_dictionary({
                "auctionId": 1,
                "slotIndex": 2,
                "accountId": 3,
                "buyToken": 4,
                "buyAmount": "6",
                "sellAmount": "7"
            })

    def test_throws_with_non_integer_value(self) -> None:
        with self.assertRaises(ValueError):
            Order.from_dictionary({
                "auctionId": "Bad Text!",
                "slotIndex": 2,
                "accountId": 3,
                "buyToken": 4,
                "sellToken": 5,
                "buyAmount": "6",
                "sellAmount": "7"
            })


class AccountRecordTest(unittest.TestCase):
    def test_to_dictionary(self) -> None:
        rec = AccountRecord(1, "Hash", [1, 2, 3])
        expected_dict = {
            "stateIndex": 1,
            "stateHash": "Hash",
            "balances": ["1", "2", "3"]
        }

        self.assertEqual(expected_dict, rec.to_dictionary())


class AuctionResultsTest(unittest.TestCase):
    def test_from_dictionary(self) -> None:
        auction_result_dict = {
            "prices": [1, 2, 3],
            "buy_amounts": [1, 3, 5],
            "sell_amounts": [0, 2, 4]
        }
        expected_res = AuctionResults([1, 2, 3], [1, 3, 5], [0, 2, 4])

        self.assertEqual(expected_res, AuctionResults.from_dictionary(auction_result_dict))

    def test_from_dict_fail_insufficient_keys(self) -> None:
        with self.assertRaises(AssertionError):
            AuctionResults.from_dictionary({
                "prices": [1, 2, 3],
                "BAD_KEY": [1, 3, 5],
                "sell_amounts": [0, 2, 4],
            })


class AuctionSettlementTest(unittest.TestCase):
    def test_from_dict(self) -> None:
        settlement_dict = {
            "auctionId": 1,
            "stateIndex": 2,
            "stateHash": "hash",
            "pricesAndVolumes": "hashed_bytes",
        }
        expected = AuctionSettlement(1, 2, "hash", "hashed_bytes")
        self.assertEqual(expected, AuctionSettlement.from_dictionary(settlement_dict))

    def test_from_dict_failure(self) -> None:
        with self.assertRaises(AssertionError):
            AuctionSettlement.from_dictionary({
                "BAD_KEY": 1,
                "stateIndex": 2,
                "stateHash": "hash",
                "pricesAndVolumes": "hashed_bytes",
            })

    def test_to_dict(self) -> None:
        rec = AuctionSettlement(1, 2, "hash", "hashed_bytes")
        expected = {
            "auctionId": 1,
            "stateIndex": 2,
            "stateHash": "hash",
            "pricesAndVolumes": "hashed_bytes",
        }
        self.assertEqual(expected, AuctionSettlement.to_dictionary(rec))

    def test_serialize_solution(self) -> None:
        num_tokens = 3

        settlement = AuctionSettlement(1, 2, "hash", "0x" + "0" * 24 * num_tokens + "0" * 24 * 2)

        serialized_solution = settlement.serialize_solution(num_tokens)
        self.assertEqual([0, 0, 0], serialized_solution.prices)
        self.assertEqual([0, 0], serialized_solution.buy_amounts)
        self.assertEqual([0], serialized_solution.sell_amounts)

    def test_serialize_solution_warning(self) -> None:
        num_tokens = 3
        settlement = AuctionSettlement(1, 2, "hash", "0x" + "0" * 24 * num_tokens + "0" * 24 * 3)

        serialized_solution = settlement.serialize_solution(num_tokens)
        self.assertEqual([0, 0, 0], serialized_solution.prices)
        self.assertEqual([0, 0], serialized_solution.buy_amounts)
        self.assertEqual([0, 0], serialized_solution.sell_amounts)


class StateTransitionTest(unittest.TestCase):
    def test_from_dict(self) -> None:
        transition_dict = {
            "transitionType": TransitionType.Deposit,
            "stateIndex": 2,
            "stateHash": "0xbdbf90e53369e96fd67d57999d2b33e28a877216d962dfac023b1234567890",
            "slot": 1,
        }
        expected = StateTransition(
            TransitionType.Deposit,
            2,
            "0xbdbf90e53369e96fd67d57999d2b33e28a877216d962dfac023b1234567890",
            1
        )
        self.assertEqual(expected, StateTransition.from_dictionary(transition_dict))

    def test_from_dict_failure(self) -> None:
        with self.assertRaises(AssertionError):
            bad_transition_dict = {
                "BAD_KEY": TransitionType.Deposit,
                "stateIndex": 2,
                "stateHash": "0x6e5066077cdaf2f0b697e15a49f624e429adeb62",
                "slot": 1,
            }
            StateTransition.from_dictionary(bad_transition_dict)

    def test_bad_hash(self) -> None:
        with self.assertRaises(AssertionError):
            bad_transition_dict = {
                "transitionType": TransitionType.Deposit,
                "stateIndex": 2,
                "stateHash": "Not A Hash",
                "slot": 1,
            }
            StateTransition.from_dictionary(bad_transition_dict)

    def test_bad_slot(self) -> None:
        with self.assertRaises(AssertionError):
            bad_transition_dict = {
                "transitionType": TransitionType.Deposit,
                "stateIndex": 2,
                "stateHash": "0x6e5066077cdaf2f0b697e15a49f624e429adeb62",
                "slot": "fart",
            }
            StateTransition.from_dictionary(bad_transition_dict)
