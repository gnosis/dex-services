from abc import ABC, abstractmethod
from pymongo import MongoClient
from django.conf import settings
from typing import Dict, List, Union

import logging

class DatabaseInterface(ABC):
    @abstractmethod
    def write_deposit(self, deposit: Dict) -> None: pass
    
    @abstractmethod
    def write_account_state(self, account_record: Dict) -> None: pass

    @abstractmethod
    def write_constants(self, num_tokens: int, num_accounts: int) -> None: pass

    @abstractmethod
    def get_account_state(self, index: int) -> Dict: pass

    @abstractmethod
    def get_deposits(self, slot: int) -> Dict: pass

    @abstractmethod
    def get_num_tokens(self) -> int: pass

class MongoDbInterface(DatabaseInterface):
    def __init__(self):
        client = MongoClient(
            host=settings.DB_HOST,
            port=settings.DB_PORT
        )
        self.db = client.get_database(settings.DB_NAME)
        self.logger = logging.getLogger(__name__)

    def write_deposit(self, event: Dict) -> None:
        deposit_id = self.db.deposits.insert_one(event).inserted_id
        self.logger.info("Successfully included Deposit - {}".format(deposit_id))

    def write_withdraw(self, event: Dict):
        withdraws = self.db.withdraws
        withdraw_id = withdraws.insert_one(event).inserted_id
        self.logger.info("Successfully included Withdraw - {}".format(withdraw_id))
    
    def write_account_state(self, account_record: Dict) -> None:
        self.db.accounts.insert_one(account_record)
    
    def write_constants(self, num_tokens: int, num_accounts: int) -> None:
        self.db.constants.insert_one({
            'num_tokens': num_tokens,
            'num_accounts': num_accounts
        })

    def get_account_state(self, index: int) -> Dict:
        return self.db.accounts.find_one({'stateIndex': index})

    def get_deposits(self, slot: int) -> Dict:
        return self.db.deposits.find({'slot': slot})
    
    def get_withdraws(self, slot: int) -> Dict:
        return self.db.withdraws.find({'slot': slot})

    def get_num_tokens(self) -> int:
        return self.db.constants.find_one()['num_tokens']