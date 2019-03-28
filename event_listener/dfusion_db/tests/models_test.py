import unittest
from ..models import Deposit, Withdraw, Order


class DepositTest(unittest.TestCase):
    def test_from_dictionary(self) -> None:
        deposit = Deposit.from_dictionary({
            "accountId": 1,
            "tokenId": 2,
            "amount": 3,
            "slot": 4,
            "slotIndex": 5
        })
        assert deposit.account_id == 1
        assert deposit.token_id == 2
        assert deposit.amount == 3
        assert deposit.slot == 4
        assert deposit.slot_index == 5

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
        assert withdraw.account_id == 1
        assert withdraw.token_id == 2
        assert withdraw.amount == 3
        assert withdraw.slot == 4
        assert withdraw.slot_index == 5

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

    def test_throws_with_missing_key(self) -> None:
        with self.assertRaises(AssertionError):
            Withdraw.from_dictionary({
                "auctionId": 1,
                "slotIndex": 2,
                "accountId": 3,
                "buyToken": 4,
                "buyAmount": 6,
                "sellAmount": 7
            })

    def test_throws_with_non_integer_value(self) -> None:
        with self.assertRaises(ValueError):
            Withdraw.from_dictionary({
                "auctionId": "Bad Text!",
                "slotIndex": 2,
                "accountId": 3,
                "buyToken": 4,
                "sellToken": 5,
                "buyAmount": 6,
                "sellAmount": 7
            })