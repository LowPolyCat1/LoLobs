const socketUrl = 'ws://127.0.0.1:2009/ws';
let socket;

function connect() {
    socket = new WebSocket(socketUrl);

    socket.onopen = () => {
        console.log("%c CONNECTED TO RUST BACKEND ", "background: #222; color: #bada55");
    };

    socket.onmessage = (event) => {
        try {
            const data = JSON.parse(event.data);

            // 1. Handle Summoner Info
            if (data.gameName) {
                const nameEl = document.getElementById('game-name');
                const tagEl = document.getElementById('tag-line');
                if (nameEl) nameEl.innerText = data.gameName;
                if (tagEl) tagEl.innerText = `#${data.tagLine || 'NA1'}`;
            }

            // 2. Handle Ranked Stats
            const soloQ = data.queues?.find(q => q.queueType === "RANKED_SOLO_5x5");
            if (soloQ) {
                const tierEl = document.getElementById('tier');
                const lpEl = document.getElementById('lp');

                if (tierEl) {
                    const division = (soloQ.division && soloQ.division !== "NA") ? soloQ.division : "";
                    tierEl.innerText = `${soloQ.tier} ${division}`.trim();
                }
                if (lpEl) lpEl.innerText = `${soloQ.leaguePoints} LP`;

                const iconImg = document.getElementById('rank-icon');
                if (iconImg && soloQ.tier) {
                    const newSrc = `https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-emblem/emblem-${soloQ.tier.toLowerCase()}.png`;
                    if (iconImg.src !== newSrc) iconImg.src = newSrc;
                }

                if (document.getElementById('wins')) {
                    const w = soloQ.wins || 0;
                    const l = soloQ.losses || 0;
                    const total = w + l;
                    const wr = total > 0 ? ((w / total) * 100).toFixed(1) : "0.0";
                    document.getElementById('wins').innerText = w;
                    document.getElementById('losses').innerText = l;
                    document.getElementById('wr-value').innerText = `${wr}%`;
                }
            }

            // 3. Handle Session Stats (Live Updates)
            if (data.session) {
                const sw = data.session.wins;
                const sl = data.session.losses;
                const sTotal = sw + sl;
                const sWr = sTotal > 0 ? ((sw / sTotal) * 100).toFixed(0) : "0";

                if (document.getElementById('s-wins')) document.getElementById('s-wins').innerText = sw;
                if (document.getElementById('s-losses')) document.getElementById('s-losses').innerText = sl;
                if (document.getElementById('s-wr')) document.getElementById('s-wr').innerText = `${sWr}%`;
            }

            // Reveal cards once data arrives
            document.querySelectorAll('.overlay-card').forEach(el => el.classList.add('visible'));

        } catch (e) {
            console.error("Overlay update error:", e);
        }
    };

    socket.onclose = () => {
        setTimeout(connect, 2000);
    };

    socket.onerror = (err) => {
        socket.close();
    };
}

connect();