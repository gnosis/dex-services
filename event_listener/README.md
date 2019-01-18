This can be tested as a standalone service as follows;


Installation
============

OS requirements
---------------

- `python3.6` (virtual environment recommended)
- [MongoDB](https://docs.mongodb.com/manual/installation/)

all other requirements are python libraries listed in `dex-services/event_listner`

```bash
git clone git@github.com:gnosis/dex-services.git
cd dex-services/event_listner
pip install -r requirements.txt
```


Testing
=======


Run ganache and mongo-db (default localhost and port settings are fine)

```bash
ganache-cli
mongod
```

Deploy a [SnappBase Contract](https://github.com/gnosis/dex-contracts) and paste its address into the SNAPP_CONTRACT_ADDRESS in `dex-services/.env`

from the dex-contracts
```bash
truffle migrate --network development
```

As an example, this yields

```
.
.
.
3_batch_auction.js
==================

   Deploying 'SnappBase'
   ---------------------
   > transaction hash:    0x7e968d2bf759c7a323173a94277e41d0fcf5ee6c563726cb85a684b947ecd41b
   > Blocks: 0            Seconds: 0
   > contract address:    0x6A28a306bE4121529F80df1d406A8Cdc5076DBfd
   > account:             0x98cC12a6c7CBA60F984B297E8e482735c8ad75C5
   > balance:             99.95211022
   > gas used:            1971591
   > gas price:           20 gwei
   > value sent:          0 ETH
   > total cost:          0.03943182 ETH
```

So our `.env` will look like this

```
SNAPP_CONTRACT_ADDRESS=0x6A28a306bE4121529F80df1d406A8Cdc5076DBfd
```

Execute the django-test as follows;

```bash
python manage.py test
```
**Note that** The test assumes that the contract events have already been emitted while, similarly, the indefinite event listener is run with

```bash
rm db.sqlite3
python manage.py migrate
python manage.py run_listener
```

Initiating Events from the Smart Contract
-----------------------------------------

From within the [dex-contracts](https://github.com/gnosis/dex-contracts) repository, with the truffle console

```
truffle console --network development
> const me = (await web3.eth.getAccounts())[0]
> const instance = await SnappBase.deployed()
> await instance.openAccount(1)
> const token = await ERC20Mintable.new()
> await instance.addToken(token.address)
> await token.mint(me, 10)
> await token.approve(instance.address, 10)
> await instance.deposit(1, 1)
```

This should yield the following log from the listener service

```
2019-01-18 13:12:15,008 [INFO] [MainProcess] Deposit received {'accountId': 1, 'tokenId': 1, 'amount': 1, 'slot': 0}
```

The database will also reflect this event!


```
mongo
> use test_db
> db.deposits.find()
{ "_id" : ObjectId("5c41d0afbda1c1620c75b1fa"), "accountId" : 1, "tokenId" : 1, "amount" : 1, "slot" : 0 }

```
