// Integer-only mirror of flow_sync. Amounts are lamports, time is chain seconds.
export function flowProjection(s,balance,stream,flow,now){
 const scale=10n**18n;now=BigInt(now);
 if(stream.virtualTime!==-1n)throw Error('Reward flow is not active');
 if(now<stream.lastWall)throw Error('Clock moved backwards');
 let budget=flow.budgetRemaining,seconds=BigInt(flow.secondsRemaining),remaining=stream.reserved,index=s.rewardIndex,released=0n;
 if(seconds<1n||seconds>60n||budget>remaining)throw Error('Invalid reward flow');
 const fresh=BigInt(balance)-s.rentFloor-s.liability-remaining;if(fresh<0n)throw Error('Vault is underfunded');
 let elapsed=s.totalWeight>0n?now-stream.lastWall:0n;
 for(let i=0;i<4601&&elapsed>0n;i++){
  if(budget===0n&&remaining/100n===0n){seconds=((seconds-elapsed%60n-1n)%60n+60n)%60n+1n;elapsed=0n;break;}
  const dt=elapsed<seconds?elapsed:seconds,amount=dt===seconds?budget:budget*dt/seconds;
  remaining-=amount;budget-=amount;released+=amount;
  elapsed-=dt;seconds-=dt;
  if(seconds===0n){seconds=60n;budget=remaining/100n;}
 }
 if(elapsed>0n)throw Error('Reward flow capacity exceeded');
 if(released>0n)index+=released*scale/s.totalWeight;
 remaining+=fresh;budget+=fresh*100n*seconds/600000n;
 return {index,unallocated:remaining,released,fresh,budgetRemaining:budget,secondsRemaining:Number(seconds),lastWall:now};
}
