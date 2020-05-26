const fs = require("fs").promises;

function encodeOrder(order, balance) {
	hex = (val, n) => val.toString(16).padStart(n * 2, "0")

	const a = order.accountID.substr(2).padStart(40, "0");
	const b = hex(BigInt(balance || 0), 32);
	const bt = hex(Number(order.buyToken.substr(1)), 2);
	const st = hex(Number(order.sellToken.substr(1)), 2);
	const vf = hex(0x00000000, 4);
	const vu = hex(0xffffffff, 4);
	const ba = hex(BigInt(order.buyAmount), 16);
	const sa = hex(BigInt(order.sellAmount), 16);
	const id = hex(Number(order.orderID), 2);
	return `${a} ${b} ${bt} ${st} ${vf} ${vu} ${ba} ${sa} ${sa} ${id}\n`
}

async function readJson(path) {
	const json = await fs.readFile(path, "utf8");
	return JSON.parse(json);
}

async function main(args) {
	if (args.length != 2) {
		console.error("USAGE: yarn convert <instance> <batch>");
		throw new Error("invalid arguments");
	}

	const instance = await readJson(args[0]);
	const batchId = parseInt(args[1]);

	const output = await fs.open(`orderbook-${batchId - 1}.hex`, "w");
	for (const order of instance.orders) {
		const balance = instance.accounts[order.accountID][order.sellToken];
		await output.write(encodeOrder(order, balance));
	}
}

main(process.argv.slice(2)).catch(err => console.log(err));
