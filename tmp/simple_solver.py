######################
# Python Simple Solver
######################

from itertools import combinations
from typing import NamedTuple, List, Dict, Set
from event_listener.dfusion_db.models import Order
from math import ceil


class TradeExecution(NamedTuple):
    buy_amount: int = 0
    sell_amount: int = 0


class Solution:

    def __init__(self, prices: Dict[int, int] = dict, amounts: Dict[Order, TradeExecution] = dict):
        self.prices: Dict[int, int] = prices
        self.amounts: Dict[Order, TradeExecution] = amounts
        self.orders: List[Order] = list(amounts.keys())
        self.surplus: Dict[Order, int] = {order: self._order_surplus(order) for order in self.orders}
        self.total_surplus: int = sum(self.surplus.values())

    def _order_surplus(self, order: Order):
        price = self.prices[order.buy_token]
        ex = self.amounts[order]
        return (ex.buy_amount - ceil(order.buy_amount * (ex.sell_amount / order.sell_amount))) * price

    def __str__(self):
        return "Prices: {prices}\nAmounts: {amounts}\nTotal Surplus: {total_surplus}".format(**self.__dict__)


def simple_solve(orders: List[Order], tokens: Set[int]) -> Solution:
    res_price = {t: 1 for t in tokens}
    res_vol = {o: TradeExecution() for o in orders}

    for x, y in list(combinations(orders, r=2)):

        match_conditions = [
            x.buy_token == y.sell_token,
            y.buy_token == x.sell_token,
            x.buy_amount * y.buy_amount <= y.sell_amount * x.sell_amount
        ]

        if all(match_conditions):

            if x.buy_amount <= y.sell_amount and x.sell_amount <= y.buy_amount:  # Type I-A (x <= y)
                res_price[x.buy_token] = x.sell_amount
                res_price[y.buy_token] = x.buy_amount
                res_vol[x] = TradeExecution(sell_amount=x.sell_amount, buy_amount=x.buy_amount)
                res_vol[y] = TradeExecution(sell_amount=x.buy_amount, buy_amount=x.sell_amount)

            elif x.buy_amount >= y.sell_amount and x.sell_amount >= y.buy_amount:  # Type I-B (x >= y)
                res_price[x.sell_token] = y.sell_amount
                res_price[y.sell_token] = y.buy_amount
                res_vol[x] = TradeExecution(sell_amount=y.buy_amount, buy_amount=y.sell_amount)
                res_vol[y] = TradeExecution(sell_amount=y.sell_amount, buy_amount=y.buy_amount)

            else:  # Type II
                res_price[x.buy_token] = y.sell_amount
                res_price[y.buy_token] = x.sell_amount
                res_vol[x] = TradeExecution(sell_amount=x.sell_amount, buy_amount=y.sell_amount)
                res_vol[y] = TradeExecution(sell_amount=y.sell_amount, buy_amount=x.sell_amount)

            return Solution(prices=res_price, amounts=res_vol)

    return Solution()
