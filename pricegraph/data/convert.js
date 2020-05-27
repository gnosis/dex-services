const fs = require("fs").promises;

function encodeOrder(order, sellTokenBalance) {
	const num = (val, n) => BigInt(val).toString(16).padStart(n * 2, "0");
	const token = (val) => num(val.substr(1), 2);
	const owner = order.accountID.substr(2).padStart(40, "0");
	return [
		owner,
		num(sellTokenBalance || 0, 32),
		token(order.buyToken),
		token(order.sellToken),
		num(0x00000000, 4),
		num(0xffffffff, 4),
		num(order.buyAmount, 16),
		num(order.sellAmount, 16),
		num(order.sellAmount, 16),
		num(order.orderID, 2),
	].join(" ") + "\n";
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

	const output = await fs.open(`orderbook-${batchId}.hex`, "w");
	for (const order of instance.orders) {
		const balance = instance.accounts[order.accountID][order.sellToken];
		await output.write(encodeOrder(order, balance));
	}
}

main(process.argv.slice(2)).catch(err => console.log(err));
