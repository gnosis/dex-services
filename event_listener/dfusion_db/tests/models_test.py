import unittest
from ..models import Deposit, Withdraw


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