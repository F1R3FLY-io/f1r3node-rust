# Helm handbook

Helm Chart for deploying F1r3fly nodes. Currently Helm Chart generate separate StatefulSets for each node. It supports up to 1000 nodes (limited by Service template now. It can be fixed if we can use 50000-60000 NodePorts or others)

## Validator keys

The chart holds no private keys. Each deployable node `<index>` reads its private key from the key `validator-<index>` of an existing Kubernetes Secret. Node 0 is the bootstrap node. Set the name of the Secret in `validatorKeys.existingSecret`. When this value is empty, `helm install` and `helm template` fail.

Each private key must match the public key `nodeKeys[<index>].publicKey` in `values.yaml`, because `bonds.txt` is rendered from these public keys. Create the Secret in the release namespace before you install the chart:
```sh
kubectl create namespace f1r3fly-4nodes
kubectl create secret generic f1r3fly-validator-keys -n f1r3fly-4nodes \
  --from-literal=validator-0=<private key of node 0> \
  --from-literal=validator-1=<private key of node 1> \
  --from-literal=validator-2=<private key of node 2> \
  --from-literal=validator-3=<private key of node 3> \
  --from-literal=validator-4=<private key of node 4>
```

## Installing Helm chart
- boot plus 4 validators
```sh
helm upgrade --install f1r3fly-4nodes ./f1r3fly -n f1r3fly-4nodes --create-namespace --set shardConfig.deployableReplicas=5 --set validatorKeys.existingSecret=f1r3fly-validator-keys
```
- boot plus 8 validators:
```sh
helm upgrade --install f1r3fly-8nodes ./f1r3fly -n f1r3fly-8nodes --create-namespace --set shardConfig.deployableReplicas=9 --set validatorKeys.existingSecret=f1r3fly-validator-keys
```
- boot plus 16 validators:
```sh
helm upgrade --install f1r3fly-16nodes ./f1r3fly -n f1r3fly-16nodes --create-namespace --set shardConfig.deployableReplicas=17 --set validatorKeys.existingSecret=f1r3fly-validator-keys
```
- boot plus 32 validators:
```sh
helm upgrade --install f1r3fly-32nodes ./f1r3fly -n f1r3fly-32nodes --create-namespace --set shardConfig.deployableReplicas=33 --set validatorKeys.existingSecret=f1r3fly-validator-keys
```


## Uninstalling Helm chart
- 4 nodes
```sh
helm uninstall f1r3fly-4nodes -n f1r3fly-4nodes
kubectl delete ns f1r3fly-4nodes
```
- 8 nodes
```sh
helm uninstall f1r3fly-8nodes -n f1r3fly-8nodes
kubectl delete ns f1r3fly-8nodes
```
- 16 nodes
```sh
helm uninstall f1r3fly-16nodes -n f1r3fly-16nodes
kubectl delete ns f1r3fly-16nodes
```

- 32 nodes
```sh
helm uninstall f1r3fly-32nodes -n f1r3fly-32nodes
kubectl delete ns f1r3fly-32nodes
```
