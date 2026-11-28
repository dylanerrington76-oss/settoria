import{j as e,l as i,r as c}from"./index-Bz48qCD3.js";const h=`### Changelog

All notable changes to this project will be documented in this file.

### [v2026.11.26]
refactored game.rs
- added the Alchemist development card (optional in the lobby): played before the roll to choose what each die shows. With it on, a 25-card deck has 12 Knights and 2 Alchemists
- added a variable discard limit setting: the limit rises by 1 every chosen number of rolls, up to a chosen cap

### [v2026.11.25]
HTTP optimizations

### [v2026.11.24]
added new map Maritime Southeast Asia.

### [v2026.11.23]
Added support for up to 12 players.

### [v2026.11.22]
- boards are sent in a compact form

### [v2026.11.21]
increased max map hex size
- refactored validate.rs god class

### [v2026.11.20]
added new hex 'gold'
- added new maps Texas and Europe.

### [v2026.11.19]
added bots 

### [v2026.11.18] 
new fairness rules
- add new stat postgame% to the end game screen
- Updated Port desgin
- Harbour owners can no longer make bank-rate deals with players
- Banned trade and mono

### [v2026.11.17]
websocket optimizations 

### [v2026.11.16]
After a trade, the Give resource now stays selected, so repeat trades only need a new Get pick.

### [v2026.11.15]
mobile ui redesign

### [v2026.11.13]
added new hex ‘volcano’

### [v2026.11.12]
added bridges

### [v2026.11.11]
Broadcast now computes longest-road lengths once per broadcast. Resource and DevCard are now enums.

### [v2026.11.10]
Added support for up to 8 players.

### [v2026.11.09]
Added a map editor for building custom maps 

### [v2026.11.08]
Updated end of game screen and added changelog page. 

### [v2026.11.06]
Updated road and settlement design. 

### [v2026.11.05] 
Maps are now loadable JSON files. Also added new map “America”
`;function g(d){const s=[];let n=null;for(const t of d.split(/\r?\n/)){const a=t.trim(),r=/^#{1,6}\s*\[v?([^\]]+)\]/.exec(a);if(r)n={date:r[1].trim(),text:"",bullets:[]},s.push(n);else if(/^#{1,6}\s/.test(a))n=null;else if(n&&a){const o=/^[-*]\s+(.*)$/.exec(a);o?n.bullets.push(o[1]):n.text=n.text?`${n.text} ${a}`:a}}return s.sort((t,a)=>a.date.localeCompare(t.date,void 0,{numeric:!0}))}const l=g(h);function p(){return e.jsx("div",{className:"cl-divider","aria-hidden":"true",children:e.jsx("svg",{viewBox:"0 0 1000 24",preserveAspectRatio:"none",children:e.jsx("path",{d:"M0 15 C120 9 230 17 350 12 S570 10 700 8 S900 3 1000 6 L1000 15 C900 19 770 17 650 19 S380 21 240 22 S60 22 0 19 Z"})})})}function u({onBack:d}){return e.jsxs("div",{className:"changelog",children:[e.jsx("div",{className:"changelog-bg","aria-hidden":"true"}),e.jsxs("a",{className:"changelog-back",href:"/",onClick:i(d),children:[e.jsx("span",{"aria-hidden":"true",children:"←"})," Back to lobby"]}),e.jsx("main",{className:"changelog-scroll",tabIndex:0,"aria-label":"Changelog",children:e.jsxs("div",{className:"changelog-inner",children:[e.jsx("h1",{children:"Changelog"}),l.length===0&&e.jsx("p",{className:"changelog-entry",children:"No changes recorded yet."}),l.map((s,n)=>e.jsxs(c.Fragment,{children:[n>0&&e.jsx(p,{}),e.jsxs("article",{className:"changelog-entry",children:[e.jsxs("p",{children:[s.date,s.text?`: ${s.text}`:":"]}),s.bullets.length>0&&e.jsx("ul",{children:s.bullets.map((t,a)=>e.jsx("li",{children:t},a))})]})]},s.date))]})})]})}export{u as default,g as parseChangelog};
