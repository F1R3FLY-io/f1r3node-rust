# SETUP

If you've run RNode previously, delete the pre-existing configuration files.

    rm -rf ~/.rnode/
    
From the `rchain` directory, build RNode.

    sbt node/universal:stage
    
From `node/target/universal/stage`, start RNode.

    ./bin/rnode run -s --wallets-file $HOME/IdeaProjects/rchain/rholang/examples/wallets.txt
    
This generates a random Secp256k1 private key corresponding to a validator. Next, terminate RNode and restart as one of 
the randomly generated validators.

    ./bin/rnode run -s --validator-private-key $(cat ~/.rnode/genesis/*.sk | tail -1) --wallets-file $HOME/IdeaProjects/rchain/rholang/examples/wallets.txt

Open a new terminal and navigate to `rholang/examples`, then add simulated user credentials to bash environment.

    . keys.env

# DEMO START

## Know your VaultAddress

Here's how Alice would check her vault address:

    ./propose.sh $ALICE_PRV vault_demo/1.know_ones_vaultaddress.rho "-e s/%PUB_KEY/$ALICE_PUB/"

## Read a wallet balance

These scripts use the native REV layer, `rho:rev:wallet`. They work only on a
shard that runs the v6 order-then-execute design.

Alice uses this command to read her wallet balance:

    ./propose.sh $ALICE_PRV vault_demo/2.check_balance.rho "-e s/%VAULT_ADDR/$ALICE_VAULT/"

Anyone can check the balance of any wallet.

    ./propose.sh $BOB_PRV vault_demo/2.check_balance.rho "-e s/%VAULT_ADDR/$ALICE_VAULT/"

The result is `(true, balance)`, or `(false, message)` when the address is not valid.

The node CLI can also read a balance:

    node wallet-balance --address $ALICE_VAULT

## Transfer to a VaultAddress

Alice uses these commands to transfer 100 tokens to Bob. The payer is always the deployer, so the
script does not take a source address.

    ./propose.sh $ALICE_PRV vault_demo/3.transfer_funds.rho "-e s/%TO/$BOB_VAULT/"
    ./propose.sh $ALICE_PRV vault_demo/2.check_balance.rho "-e s/%VAULT_ADDR/$ALICE_VAULT/"

The node CLI makes the same transfer:

    node transfer --to $BOB_VAULT --amount 100 --phlo-limit 100000 --phlo-price 1 --private-key $ALICE_PRV

Bob then reads his own balance:

    ./propose.sh $BOB_PRV vault_demo/2.check_balance.rho "-e s/%VAULT_ADDR/$BOB_VAULT/"

A transfer debits the wallet of the deployer and adds a separate deposit to the wallet of the
payee. The payee does not have to create a wallet first. Every address has a wallet, and its
balance is zero until a deposit arrives.

## Attempt a transfer despite insufficient funds

    ./propose.sh $ALICE_PRV vault_demo/3.transfer_funds.rho "-e s/%TO/$BOB_VAULT/"

The result is `(false, "Insufficient funds")`, and no wallet changes.

## Attempt a transfer despite invalid VaultAddress

    ./propose.sh $ALICE_PRV vault_demo/3.transfer_funds.rho "-e s/%TO/lala/"

The result is `(false, message)`, and no wallet changes. The platform checks only the syntax and
the checksum of the address. A valid address with a typo sends the funds to a different wallet.
