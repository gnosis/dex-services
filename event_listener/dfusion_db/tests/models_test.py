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
            "valid": False,
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

    def test_from_bytes(self) -> None:
        price_strings = list(map(lambda x: str(hex(x))[2:].rjust(24, "0"), [1, 2, 3]))
        amount_strings = list(map(lambda x: str(hex(x))[2:].rjust(24, "0"), [1, 2, 3, 4]))
        solution_bytes = "".join(price_strings) + "".join(amount_strings)
        solution = AuctionResults.from_bytes(solution_bytes, 3)
        self.assertEqual(solution.prices, [1, 2, 3], "Solution's prices unexpected")
        self.assertEqual(solution.buy_amounts, [1, 3], "Solution's buy amounts unexpected")
        self.assertEqual(solution.sell_amounts, [2, 4], "Solution's sell amounts unexpected")

    def test_bad_bytes(self) -> None:
        price_strings = list(map(lambda x: str(hex(x))[2:].rjust(24, "0"), [1, 2, 3]))
        # Amount list should have even length (i.e. sell amount for every buy amount)!
        bad_amount_strings = list(map(lambda x: str(hex(x))[2:].rjust(24, "0"), [1, 2, 3]))
        bad_bytes = "".join(price_strings) + "".join(bad_amount_strings)

        with self.assertLogs():
            AuctionResults.from_bytes(bad_bytes, 3)


class AuctionSettlementTest(unittest.TestCase):

    NUM_TOKENS = 3
    EMPTY_SOLUTION_BYTES = "0" * 24 * NUM_TOKENS + "0" * 24 * 2
    AUCTION_RESULTS = AuctionResults.from_bytes(EMPTY_SOLUTION_BYTES, NUM_TOKENS)

    def test_from_dict(self) -> None:
        settlement_dict = {
            "auctionId": 1,
            "stateIndex": 2,
            "stateHash": "hash",
            "pricesAndVolumes": self.EMPTY_SOLUTION_BYTES,
        }

        expected = AuctionSettlement(1, 2, "hash", self.AUCTION_RESULTS)
        self.assertEqual(expected, AuctionSettlement.from_dictionary(settlement_dict, self.NUM_TOKENS))

    def test_from_dict_failure(self) -> None:
        with self.assertRaises(AssertionError):
            AuctionSettlement.from_dictionary({
                "BAD_KEY": 1,
                "stateIndex": 2,
                "stateHash": "hash",
                "pricesAndVolumes": "hashed_bytes",
            }, self.NUM_TOKENS)


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
