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
            "valid": self.valid,
            "id": self.id
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
    def from_dictionary(cls, data: Dict[str, List[int]]) -> "AuctionResults":
        event_fields = ('prices', 'buy_amounts', 'sell_amounts')
        assert all(k in data for k in event_fields), "Unexpected keys. Got {}".format(data.keys())

        return AuctionResults(
            data["prices"],
            data["buy_amounts"],
            data["sell_amounts"]
        )


class AuctionSettlement(NamedTuple):
    auction_id: int
    state_index: int
    state_hash: str
    prices_and_volumes: str  # TODO - Emitted as Hex String  # Should be a Prices & Buy Amounts, Sell Amounts

    @classmethod
    def from_dictionary(cls, data: Dict[str, Any]) -> "AuctionSettlement":
        event_fields = ('auctionId', 'stateIndex', 'stateHash', 'pricesAndVolumes')
        assert all(k in data for k in event_fields), "Unexpected Event Keys: got {}".format(data.keys())
        return AuctionSettlement(
            int(data['auctionId']),
            int(data['stateIndex']),
            str(data['stateHash']),
            str(data['pricesAndVolumes']),  # TODO - Call serialize solution
        )

    def to_dictionary(self) -> Dict[str, Any]:
        return {
            "auctionId": self.auction_id,
            "stateIndex": self.state_index,
            "stateHash": self.state_hash,
            "pricesAndVolumes": self.prices_and_volumes,
        }

    def serialize_solution(self, num_tokens: int) -> AuctionResults:
        """Transform Byte Code for prices_and_volumes into Prices & TradeExecution objects"""
        logging.info("Serializing Auction Results (from bytecode)")

        # TODO - pass num_orders (as read from DB for solution verification)
        hex_str_array = [self.prices_and_volumes[i: i + 24] for i in range(0, len(self.prices_and_volumes), 24)]
        byte_array = list(map(lambda t: int(t, 16), hex_str_array))

        prices, volumes = byte_array[:num_tokens], byte_array[num_tokens:]
        buy_amounts = volumes[0::2]  # Even elements
        sell_amounts = volumes[1::2]  # Odd elements

        if len(buy_amounts) != len(sell_amounts):
            # TODO - assert that buy and sell amounts have same length and are less than num_orders
            logging.warning("Solution data is not correct!")

        return AuctionResults.from_dictionary({
            "prices": prices,
            "buy_amounts": buy_amounts,
            "sell_amounts": sell_amounts,
        })
