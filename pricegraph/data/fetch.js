const Web3 = require("web3");
const fs = require("fs").promises;
const { BatchExchange, BatchExchangeViewer } = require("@gnosis.pm/dex-contracts");

const ALL_TOKENS = [];
const ORDER_STRIDE = 224;

function formatElement(element) {
	const e = i => {
		s = element.substring(0, i);
		element = element.substring(i);
		return s;
	};

	return `${e(40)} ${e(64)} ${e(4)} ${e(4)} ${e(8)} ${e(8)} ${e(32)} ${e(32)} ${e(32)}\n`;
}

function contract(web3, artifact) {
	return new web3.eth.Contract(artifact.abi, artifact.networks["1"].address);
}

async function writeElements(file, elements) {
	elements = elements.substring(2);
	while (elements.length > 0) {
		const element = elements.substring(0, ORDER_STRIDE);
		await file.write(formatElement(element));
		elements = elements.substring(ORDER_STRIDE);
	}
}

async function main() {
	const web3 = new Web3("https://node.mainnet.gnosisdev.com");
	const exchange = contract(web3, BatchExchange);
	const viewer = contract(web3, BatchExchangeViewer);

	const batchId = await exchange.methods.getCurrentBatchId().call();
	const output = await fs.open(`orderbook-${batchId - 1}.hex`, "w");

	const pageSize = 128;
	let page = {
		nextPageUser: "0x0000000000000000000000000000000000000000",
		nextPageUserOffset: 0,
	};

	do {
		page = await viewer
			.methods
			.getFinalizedOrderBookPaginated(
				ALL_TOKENS,
				page.nextPageUser,
				page.nextPageUserOffset,
				pageSize,
			)
			.call();
		await writeElements(output, page.elements);
	}
	while (page.hasNextPage);
}

main().catch(err => console.log(err));
