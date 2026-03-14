const socket = new WebSocket('ws://127.0.0.1:8080/ws');

socket.onmessage = (event) => {
    try {
        const data = JSON.parse(event.data);
        const soloQ = data.queues?.find(q => q.queueType === "RANKED_SOLO_5x5");

        if (data.gameName && document.getElementById('game-name')) {
            document.getElementById('game-name').innerText = data.gameName;
            document.getElementById('tag-line').innerText = `#${data.tagLine}`;
        }

        if (soloQ) {
            if (document.getElementById('tier')) {
                const division = (soloQ.division && soloQ.division !== "NA") ? soloQ.division : "";
                document.getElementById('tier').innerText = `${soloQ.tier} ${division}`.trim();
                document.getElementById('lp').innerText = `${soloQ.leaguePoints} LP`;

                const iconImg = document.getElementById('rank-icon');
                const newSrc = `https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-emblem/emblem-${soloQ.tier.toLowerCase()}.png`;
                if (iconImg && iconImg.src !== newSrc) iconImg.src = newSrc;
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

        if (data.session && document.getElementById('s-wins')) {
            const sw = data.session.wins;
            const sl = data.session.losses;
            const sTotal = sw + sl;
            const sWr = sTotal > 0 ? ((sw / sTotal) * 100).toFixed(0) : "0";
            document.getElementById('s-wins').innerText = sw;
            document.getElementById('s-losses').innerText = sl;
            document.getElementById('s-wr').innerText = `${sWr}%`;
        }

        document.querySelectorAll('.overlay-card').forEach(el => el.classList.add('visible'));
    } catch (e) {
        console.error("Overlay update error:", e);
    }
};