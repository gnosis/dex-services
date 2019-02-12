from abc import ABC, abstractmethod
from pymongo import MongoClient
from django.conf import settings
from typing import Dict, List
from .models import Deposit, StateTransition, TransitionType, Withdraw, AccountRecord

import logging


class DatabaseInterface(ABC):
    @abstractmethod
    def write_deposit(self, deposit: Deposit) -> None: pass

    @abstractmethod
    def write_withdraw(self, withdraw: Withdraw) -> None: pass

    @abstractmethod
    def write_account_state(self, account_record: AccountRecord) -> None: pass

    @abstractmethod
    def write_constants(self, num_tokens: int, num_accounts: int) -> None: pass

    @abstractmethod
    def get_account_state(self, index: int) -> AccountRecord: pass

    @abstractmethod
    def get_deposits(self, slot: int) -> List[Deposit]: pass

    @abstractmethod
    def get_withdraws(self, slot: int) -> List[Withdraw]: pass

    @abstractmethod
    def get_num_tokens(self) -> int: pass


class MongoDbInterface(DatabaseInterface):
    def __init__(self) -> None:
        client = MongoClient(
            host=settings.DB_HOST,
            port=settings.DB_PORT
        )
        self.db = client.get_database(settings.DB_NAME)
        self.logger = logging.getLogger(__name__)

    def write_deposit(self, deposit: Deposit) -> None:
        event = {
            "accountId": deposit.account_id,
            "tokenId": deposit.token_id,
            "amount": deposit.amount,
            "slot": deposit.slot,
            "slotIndex": deposit.slot_index
        }
        deposit_id = self.db.deposits.insert_one(event).inserted_id
        self.logger.info(
            "Successfully included Deposit - {}".format(deposit_id))

    def write_withdraw(self, withdraw: Withdraw) -> None:
        withdraws = self.db.withdraws
        event = {
            "accountId": withdraw.account_id,
            "tokenId": withdraw.token_id,
            "amount": withdraw.amount,
            "slot": withdraw.slot,
            "slotIndex": withdraw.slot_index
        }
        withdraw_id = withdraws.insert_one(event).inserted_id
        self.logger.info(
            "Successfully included Withdraw - {}".format(withdraw_id))

    def write_account_state(self, account_record: AccountRecord) -> None:
        record = {
            "stateIndex": account_record.state_index,
            "stateHash": account_record.state_hash,
            "balances": account_record.balances
        }
        self.db.accounts.insert_one(record)

    def write_constants(self, num_tokens: int, num_accounts: int) -> None:
        self.db.constants.insert_one({
            'num_tokens': num_tokens,
            'num_accounts': num_accounts
        })

    def get_account_state(self, index: int) -> AccountRecord:
        record = self.db.accounts.find_one({'stateIndex': index})
        return AccountRecord(record["stateIndex"], record["stateHash"], record["balances"])

    def get_deposits(self, slot: int) -> List[Deposit]:
        return list(map(lambda d: Deposit.from_dictionary(d), self.db.deposits.find({'slot': slot})))

    def get_withdraws(self, slot: int) -> List[Withdraw]:
        return list(map(lambda d: Withdraw.from_dictionary(d), self.db.withdraws.find({'slot': slot})))

    def get_num_tokens(self) -> int:
        return int(self.db.constants.find_one()['num_tokens'])
