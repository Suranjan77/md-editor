import { execSync } from 'node:child_process';

function run(cmd) {
  console.log(`\n$ ${cmd}`);
  execSync(cmd, { stdio: 'inherit' });
}

console.log('Running lightweight performance readiness checks...');
run('npm run build');
run('npm test');
console.log('\nPerformance readiness checks completed.');
console.log('Tip: run desktop profiling manually for memory leak validation in long sessions.');
