// This is an internal test script

function testMatch(text) {
    let inCodeBlock = false;
    let isFence = false;

    const fenceMatch = text.match(/^(\s*)(`{3,}|~{3,})/);
    if (fenceMatch) {
        const fenceChar = fenceMatch[2].charAt(0);
        const infoString = text.slice(fenceMatch[0].length);
        if (!infoString.includes(fenceChar)) {
            isFence = true;
            inCodeBlock = fenceChar;
        }
    }

    return { isFence, inCodeBlock };
}

console.log("```bash", testMatch("```bash")); // true
console.log("   ```", testMatch("   ```")); // true
console.log("~~~", testMatch("~~~")); // true
console.log("```sudo apt get install```", testMatch("```sudo apt get install```")); // false (contains backticks)
console.log("~~~bash~~~", testMatch("~~~bash~~~")); // false
console.log("`code`", testMatch("`code`")); // false (less than 3)

