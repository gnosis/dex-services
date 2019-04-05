######################
# Python Simple Solver
######################

from itertools import combinations
from typing import NamedTuple, List, Dict, Set
from event_listener.dfusion_db.models import Order


class TradeExecution(NamedTuple):
    buy_amount: int = 0
    sell_amount: int = 0


class Solution(NamedTuple):
    prices: Dict[int, int]
    amounts: Dict[Order, TradeExecution]


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

            elif x.buy_amount >= y.sell_amount and x.sell_amount >= y.buy_amount:  # Type I-B (y <= x)
                res_price[x.buy_token] = y.sell_amount
                res_price[y.buy_token] = y.buy_amount
                res_vol[x] = TradeExecution(sell_amount=y.sell_amount, buy_amount=y.buy_amount)
                res_vol[y] = TradeExecution(sell_amount=y.buy_amount, buy_amount=y.sell_amount)

            else:  # Type II
                res_price[x.buy_token] = y.sell_amount
                res_price[y.buy_token] = x.sell_amount
                res_vol[x] = TradeExecution(sell_amount=x.sell_amount, buy_amount=y.sell_amount)
                res_vol[y] = TradeExecution(sell_amount=y.sell_amount, buy_amount=x.sell_amount)

            break

    return Solution(prices=res_price, amounts=res_vol)
