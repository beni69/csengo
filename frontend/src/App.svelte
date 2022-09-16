<script lang="ts">
    import { z } from "zod";

    const dateZ = z.preprocess(arg => {
        if (typeof arg == "string" || arg instanceof Date) return new Date(arg);
    }, z.date());
    const Form = z.discriminatedUnion("now", [
        z.object({
            now: z.literal(true),
            name: z.string().min(1),
            time: dateZ.nullable(),
        }),
        z.object({
            now: z.literal(false),
            name: z.string().min(1),
            time: dateZ,
        }),
    ]);

    let form: z.infer<typeof Form> = {
        name: "",
        now: true,
        time: null,
    };
    const submit = () => {
        const res = Form.safeParse(form);
        res.success || alert("invalid");
        alert(JSON.stringify(form));
    };

    const fetchData = () =>
        fetch("/api/tasks")
            .then(r => r.json())
            .then(d => d.map(Form.parse));
    const data = fetchData();
    data.then(console.debug).catch(console.error);
</script>

<main>
    <div class="card">
        <h1>Új csengetés</h1>
        <form on:submit|preventDefault={submit}>
            <input type="text" bind:value={form.name} placeholder="Név" />
            <label>
                Lejátszás most
                <input type="checkbox" name="now" bind:checked={form.now} />
            </label>
            {#if !form.now}
                <input
                    type="datetime-local"
                    name="time"
                    bind:value={form.time}
                />
            {/if}
            <input type="submit" value="Go" class="btn" />
        </form>
        <p>{JSON.stringify(form)}</p>
    </div>
    <div class="card">
        <h1>Következő csengetések</h1>
        <!-- <p>
            Visit <a href="https://kit.svelte.dev">kit.svelte.dev</a> to read the
            documentation
        </p> -->
        {#await data}
            <p>loading</p>
        {:then data}
            {#each data as item}
                <p>{JSON.stringify(item)}</p>
            {/each}
        {:catch}
            <p>fetch failed</p>
        {/await}
    </div>
</main>

<style>
    :global(:root) {
        width: 100vw;
        height: 100vh;
        overflow: hidden;

        font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
        font-size: 16px;
        line-height: 24px;
        font-weight: 400;
        text-align: center;

        color-scheme: light dark;
        background: rgba(9, 9, 121, 1);
        background: linear-gradient(
            315deg,
            rgba(2, 0, 36, 1) 0%,
            rgba(9, 9, 121, 1) 35%,
            rgba(0, 212, 255, 1) 100%
        );

        display: flex;
        align-items: center;
        justify-content: space-evenly;
        flex-direction: column;
    }

    .card {
        backdrop-filter: saturate(180%) blur(10px);
        background-color: rgba(255, 255, 255, 0.4);

        /* border: 1px solid red; */
        border-radius: 8px;

        padding: 10px;
        margin: 2rem;

        min-width: 33vw;
        max-width: calc(100vw - 50px);
    }

    form {
        display: flex;
        flex-direction: column;
        align-items: center;
    }

    input {
        border: 2px solid rgb(0, 90, 255);
        border-radius: 8px;
        padding: 0.3rem;
        margin: 5px;
        background: #121212;
        color: #eee;
    }

    .btn {
        /* background-image: linear-gradient(
            to right,
            #1a2980 0%,
            #26d0ce 51%,
            #1a2980 100%
        ); */
        background-image: linear-gradient(
            to right,
            #00d2ff 0%,
            #3a7bd5 51%,
            #00d2ff 100%
        );
        padding: 15px 45px;
        text-align: center;
        text-transform: uppercase;
        transition: 0.5s;
        background-size: 200% auto;
        color: white;
    }

    .btn:hover {
        background-position: right center; /* change the direction of the change here */
        color: #fff;
        text-decoration: none;
        box-shadow: 0 0 20px #eee;
    }
</style>
