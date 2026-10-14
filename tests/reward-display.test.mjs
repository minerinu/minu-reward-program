import {test} from 'node:test';import assert from 'node:assert/strict';import {formatSol} from '../client/staking.mjs';
test('every positive sub-micro-SOL reward is visibly nonzero',()=>{for(let n=1n;n<1000n;n++)assert(Number(formatSol(n))>0,`Reward hidden at ${n} lamports`);});
test('tiny claims retain their actual lamport value',()=>{assert.equal(formatSol(14n),'0.000000014');assert.equal(formatSol(999n),'0.000000999');assert.equal(formatSol(0n),'0');});
test('ordinary SOL amounts retain the existing compact display',()=>{assert.equal(formatSol(1000n),'0.000001');assert.equal(formatSol(105505000n),'0.105505');assert.equal(formatSol(1085971000n),'1.085971');});
