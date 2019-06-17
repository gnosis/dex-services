import logging
from abc import ABC, abstractmethod
from typing import List

from django.conf import settings
from pymongo import MongoClient

from .models import Deposit, Withdraw, AccountRecord, Order, StandingOrder


class DatabaseInterface(ABC):
    @abstractmethod
    def write_deposit(self, deposit: Deposit) -> None: pass

    @abstractmethod
    def write_withdraw(self, withdraw: Withdraw) -> None: pass

    @abstractmethod
    def update_withdraw(self, old: Withdraw, new: Withdraw) -> None: pass

    @abstractmethod
    def write_account_state(self, account_record: AccountRecord) -> None: pass

    @abstractmethod
    def write_snapp_constants(self, num_tokens: int, num_accounts: int) -> None: pass

    @abstractmethod
    def write_auction_constants(
        self, num_orders: int, num_reserved_accounts: int, orders_per_reserved_account: int
    ) -> None: pass

    @abstractmethod
    def get_account_state(self, index: int) -> AccountRecord: pass

    @abstractmethod
    def get_deposits(self, slot: int) -> List[Deposit]: pass

    @abstractmethod
    def get_withdraws(self, slot: int) -> List[Withdraw]: pass

    @abstractmethod
    def get_num_tokens(self) -> int: pass

    @abstractmethod
    def get_num_orders(self) -> int: pass

    @abstractmethod
    def write_order(self, order: Order) -> None: pass

    @abstractmethod
    def get_orders(self, auction_id: int) -> List[Order]: pass

    @abstractmethod
    def write_standing_order(self, standing_order: StandingOrder) -> None: pass


class MongoDbInterface(DatabaseInterface):
    def __init__(self) -> None:
        client = MongoClient(
            host=settings.DB_HOST,
            port=settings.DB_PORT
        )
        self.database = client.get_database(settings.DB_NAME)
        self.logger = logging.getLogger(__name__)

    def write_deposit(self, deposit: Deposit) -> None:
        deposit_id = self.database.deposits.insert_one(deposit.to_dictionary()).inserted_id
        self.logger.info(
            "Successfully included Deposit - {}".format(deposit_id))

    def write_withdraw(self, withdraw: Withdraw) -> None:
        withdraw_id = self.database.withdraws.insert_one(withdraw.to_dictionary()).inserted_id
        self.logger.info(
            "Successfully included Withdraw - {}".format(withdraw_id))

    def update_withdraw(self, old: Withdraw, new: Withdraw) -> None:
        self.database.withdraws.replace_one({'_id': old.id}, new.to_dictionary())
        self.logger.info(
            "Successfully updated Withdraw - {}".format(old.id))

    def write_account_state(self, account_record: AccountRecord) -> None:
        self.database.accounts.insert_one(account_record.to_dictionary())

    def write_snapp_constants(self, num_tokens: int, num_accounts: int) -> None:
        self.database.constants.insert([
            {
                'name': 'num_tokens',
                'value': num_tokens
            },
            {
                'name': 'num_accounts',
                'value': num_accounts
            }
        ])

    def write_auction_constants(
            self, num_orders: int, num_reserved_accounts: int, orders_per_reserved_account: int
    ) -> None:
        self.database.constants.insert([
            {
                'name': 'num_orders',
                'value': num_orders
            },
            {
                'name': 'num_reserved_accounts',
                'value': num_reserved_accounts
            },
            {
                'name': 'orders_per_reserved_account',
                'value': orders_per_reserved_account
            },
        ])

    def get_account_state(self, index: int) -> AccountRecord:
        record = self.database.accounts.find_one({'stateIndex': index})
        return AccountRecord(record["stateIndex"], record["stateHash"], list(map(int, record["balances"])))

    def get_deposits(self, slot: int) -> List[Deposit]:
        return list(map(Deposit.from_dictionary, self.database.deposits.find({'slot': slot})))

    def get_withdraws(self, slot: int) -> List[Withdraw]:
        return list(map(Withdraw.from_dictionary, self.database.withdraws.find({'slot': slot})))

    def get_num_tokens(self) -> int:
        return int(self.database.constants.find_one({'name': 'num_tokens'})['value'])

    def get_num_accounts(self) -> int:
        return int(self.database.constants.find_one({'name': 'num_accounts'})['value'])

    def get_num_orders(self) -> int:
        return int(self.database.constants.find_one({'name': 'num_orders'})['value'])

    def write_order(self, order: Order) -> None:
        order_id = self.database.orders.insert_one(order.to_dictionary()).inserted_id
        self.logger.info(
            "Successfully included Order - {}".format(order_id))

    def get_orders(self, auction_id: int) -> List[Order]:
        orders = list(map(Order.from_dictionary, self.database.orders.find({'auctionId': auction_id})))

        pipeline = [
            {"$match": {"validFromAuctionId": {"$lte": auction_id}}},
            {"$sort": {"validFromAuctionId": -1, "_id": -1}},
            {"$group": {
                "_id": "$accountId",
                "batchIndex": {"$first": "$batchIndex"},
                "validFromAuctionId": {"$first": "$validFromAuctionId"},
                "orders": {"$first": "$orders"}
            }}
        ]
        standing_orders = list(map(StandingOrder.from_db_dictionary, self.database.standing_orders.aggregate(pipeline)))
        for standing_order in standing_orders:
            orders += standing_order.get_orders()
        return orders

    def write_standing_order(self, standing_order: StandingOrder) -> None:
        standing_order_id = self.database.standing_orders.insert_one(standing_order.to_dictionary()).inserted_id
        self.logger.info(
            "Successfully included StandingOrder {}".format(standing_order_id))
