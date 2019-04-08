from enum import Enum
from typing import NamedTuple, Dict, Any, List, Optional


class TransitionType(Enum):
    Deposit = 0
    Withdraw = 1
    Auction = 2


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
    amount: str
    slot: int
    slot_index: int

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Deposit":
        event_fields = ('accountId', 'tokenId', 'amount', 'slot', 'slotIndex')
        assert all(k in data for k in event_fields), "Unexpected Event Keys"
        return Deposit(
            int(data['accountId']),
            int(data['tokenId']),
            data['amount'],
            int(data['slot']),
            int(data['slotIndex'])
        )


class Withdraw(NamedTuple):
    account_id: int
    token_id: int
    amount: str
    slot: int
    slot_index: int
    valid: bool = False
    id: Optional[str] = None

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Withdraw":
        event_fields = ('accountId', 'tokenId', 'amount', 'slot', 'slotIndex')
        assert all(k in data for k in event_fields), "Unexpected Event Keys"
        return Withdraw(
            int(data['accountId']),
            int(data['tokenId']),
            data['amount'],
            int(data['slot']),
            int(data['slotIndex']),
            bool(data.get('valid', False)),
            data.get('_id', None)
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "accountId": self.account_id,
            "tokenId": self.token_id,
            "amount": self.amount,
            "slot": self.slot,
            "slotIndex": self.slot_index,
            "valid": self.valid
        }


class AccountRecord(NamedTuple):
    state_index: int
    state_hash: str
    balances: List[str]


class Order(NamedTuple):
    slot: int
    slot_index: int
    account_id: int
    buy_token: int
    sell_token: int
    buy_amount: str
    sell_amount: str

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "Order":
        event_fields = ('auctionId', 'slotIndex', 'accountId', 'buyToken', 'sellToken', 'buyAmount', 'sellAmount')
        assert all(k in data for k in event_fields), "Unexpected Event Keys"
        return Order(
            int(data['auctionId']),
            int(data['slotIndex']),
            int(data['accountId']),
            int(data['buyToken']),
            int(data['sellToken']),
            data['buyAmount'],
            data['sellAmount'],
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "auctionId": self.slot,
            "slotIndex": self.slot_index,
            "accountId": self.account_id,
            "buyToken": self.buy_token,
            "sellToken": self.sell_token,
            "buyAmount": self.buy_amount,
            "sellAmount": self.sell_amount
        }
