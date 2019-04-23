import logging
from enum import Enum
from typing import NamedTuple, Dict, Any, List, Optional


class TransitionType(Enum):
    Deposit = 0
    Withdraw = 1


class StateTransition(NamedTuple):
    transition_type: TransitionType
    state_index: int
    state_hash: str
    slot: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "StateTransition":
        assert data.keys() == {'transitionType', 'stateIndex', 'stateHash', 'slot'}, \
            "Unexpected Event Keys: got {}".format(data.keys())
        _type = TransitionType(data['transitionType'])
        assert isinstance(data['stateIndex'], int), "Transition to has unexpected values"
        _hash = data['stateHash']
        assert isinstance(_hash, str) and len(_hash) == 64, "Transition from has unexpected values"
        assert isinstance(data['slot'], int), "Transition slot not recognized"
        return StateTransition(_type, data['stateIndex'], _hash, data['slot'])


class Deposit(NamedTuple):
    account_id: int
    token_id: int
    amount: int
    slot: int
    slot_index: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Deposit":
        event_fields = ('accountId', 'tokenId', 'amount', 'slot', 'slotIndex')
        assert all(k in data for k in event_fields), "Unexpected Event Keys: got {}".format(data.keys())
        return Deposit(
            int(data['accountId']),
            int(data['tokenId']),
            int(data['amount']),
            int(data['slot']),
            int(data['slotIndex'])
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "accountId": self.account_id,
            "tokenId": self.token_id,
            "amount": str(self.amount),
            "slot": self.slot,
            "slotIndex": self.slot_index
        }


class Withdraw(NamedTuple):
    account_id: int
    token_id: int
    amount: int
    slot: int
    slot_index: int
    valid: bool = False
    id: Optional[str] = None

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Withdraw":
        event_fields = ('accountId', 'tokenId', 'amount', 'slot', 'slotIndex')
        assert all(k in data for k in event_fields), "Unexpected Event Keys: got {}".format(data.keys())
        return Withdraw(
            int(data['accountId']),
            int(data['tokenId']),
            int(data['amount']),
            int(data['slot']),
            int(data['slotIndex']),
            bool(data.get('valid', False)),
            data.get('_id', None)
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "accountId": self.account_id,
            "tokenId": self.token_id,
            "amount": str(self.amount),
            "slot": self.slot,
            "slotIndex": self.slot_index,
            "valid": self.valid
        }


class AccountRecord(NamedTuple):
    state_index: int
    state_hash: str
    balances: List[int]

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "stateIndex": self.state_index,
            "stateHash": self.state_hash,
            "balances": list(map(str, self.balances))
        }


class Order(NamedTuple):
    slot: int
    slot_index: int
    account_id: int
    buy_token: int
    sell_token: int
    buy_amount: int
    sell_amount: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Order":
        event_fields = ('auctionId', 'slotIndex', 'accountId', 'buyToken', 'sellToken', 'buyAmount', 'sellAmount')
        assert all(k in data for k in event_fields), "Unexpected Event Keys: got {}".format(data.keys())
        return Order(
            int(data['auctionId']),
            int(data['slotIndex']),
            int(data['accountId']),
            int(data['buyToken']),
            int(data['sellToken']),
            int(data['buyAmount']),
            int(data['sellAmount']),
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "auctionId": self.slot,
            "slotIndex": self.slot_index,
            "accountId": self.account_id,
            "buyToken": self.buy_token,
            "sellToken": self.sell_token,
            "buyAmount": str(self.buy_amount),
            "sellAmount": str(self.sell_amount)
        }


class AuctionResults(NamedTuple):
    prices: List[int]
    buy_amounts: List[int]
    sell_amounts: List[int]

    @classmethod
    def from_bytes(cls, byte_str: str, num_tokens: int) -> "AuctionResults":
        # TODO - pass num_orders (as read from DB for solution verification)
        hex_str_array = [byte_str[i: i+24] for i in range(0, len(byte_str), 24)]
        byte_array = list(map(lambda t: int(t, 16), hex_str_array))
        prices, volumes = byte_array[:num_tokens], byte_array[num_tokens:]
        buy_amounts = volumes[0::2]  # Even elements
        sell_amounts = volumes[1::2]  # Odd elements

        if len(buy_amounts) != len(sell_amounts):
            # TODO - ensure buy and sell amounts have same length (and < num_orders)
            logging.warning("Solution data is not correct!")

        return AuctionResults(prices, buy_amounts, sell_amounts)


class AuctionSettlement(NamedTuple):
    auction_id: int
    state_index: int
    state_hash: str
    prices_and_volumes: AuctionResults  # Emitted as Hex String

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any], num_tokens: int) -> "AuctionSettlement":
        event_fields = ('auctionId', 'stateIndex', 'stateHash', 'pricesAndVolumes')
        assert all(k in data for k in event_fields), "Unexpected Event Keys: got {}".format(data.keys())
        return AuctionSettlement(
            int(data['auctionId']),
            int(data['stateIndex']),
            str(data['stateHash']),
            AuctionResults.from_bytes(data['pricesAndVolumes'], num_tokens),
        )
