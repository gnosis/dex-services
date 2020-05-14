const Web3 = require("web3");
const fs = require("fs").promises;
const {
	BatchExchangeArtifact,
	BatchExchangeViewerArtifact,
	deployment,
	getOpenOrders,
} = require("@gnosis.pm/dex-contracts");

const ALL_TOKENS = [];
const CONFIRMATIONS = 6;
const ORDER_STRIDE = 228;

function formatElement(element) {
	const e = i => {
		s = element.substring(0, i);
		element = element.substring(i);
		return s;
	};

	return `${e(40)} ${e(64)} ${e(4)} ${e(4)} ${e(8)} ${e(8)} ${e(32)} ${e(32)} ${e(32)} ${e(4)}\n`;
}

async function writeElements(file, elements) {
	elements = (elements || "0x").substring(2);
	while (elements.length > 0) {
		const element = elements.substring(0, ORDER_STRIDE);
		await file.write(formatElement(element));
		elements = elements.substring(ORDER_STRIDE);
	}
}

async function main() {
	const { INFURA_PROJECT_ID } = process.env;
	const web3 = new Web3(`https://mainnet.infura.io/v3/${INFURA_PROJECT_ID}`);

	const [exchange] = await deployment(web3, BatchExchangeArtifact);
	const [viewer] = await deployment(web3, BatchExchangeViewerArtifact);

	const latestBlock = await web3.eth.getBlockNumber();
	const blockNumber = latestBlock - CONFIRMATIONS;

	const batchId = await exchange.methods.getCurrentBatchId().call(undefined, blockNumber);
	const output = await fs.open(`orderbook-${batchId - 1}.hex`, "w");

	const pageSize = 50;
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
			.call(undefined, blockNumber);
		await writeElements(output, page.elements);
	}
	while (page.hasNextPage);
}

main().catch(err => console.log(err));
