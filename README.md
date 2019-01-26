This can be tested as a standalone service as follows;


Installation
============

OS requirements
---------------

Clone the repository and run the container

```bash
git clone git@github.com:gnosis/dex-services.git
cd dex-services
docker-compose up
```


Deploy a [SnappBase Contract](https://github.com/gnosis/dex-contracts) and paste its address into the SNAPP_CONTRACT_ADDRESS in `dex-services/.env`

from the dex-contracts
```bash
truffle migrate --network development
```

This should yield

```
.
.
.
Deploying 'SnappBase'
   ---------------------
   > transaction hash:    0xf95c4f1b080b65714095808269065e2a95557865502b85eba611f5fa54d001e3
   > Blocks: 0            Seconds: 0
   > contract address:    0xC89Ce4735882C9F0f0FE26686c53074E09B0D550
   > account:             0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1
   > balance:             99.94782536
   > gas used:            2185834
   > gas price:           20 gwei
   > value sent:          0 ETH
   > total cost:          0.04371668 ETH

```



Execute the django-test as follows;

Restart the event listener to reflect the change in environment variables.

```bash
docker-compose restart listener
```

And then start the driver by:
```bash
cd driver
cargo run
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
